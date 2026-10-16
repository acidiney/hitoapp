use crate::catalog::Catalog;
use cap_fs_ext::{FollowSymlinks, OpenOptionsFollowExt};
use cap_std::fs::{Dir, OpenOptions};
use sqlx::{ConnectOptions, Connection, Row, SqliteConnection, sqlite::SqliteConnectOptions};
use std::{fs::File, io, path::Path, sync::Arc};

pub(crate) struct Registry {
    connection: SqliteConnection,
    claim: Arc<Claim>,
    initialized: bool,
}

pub(crate) struct Claim {
    directory: Dir,
    _lock: File,
}

impl Registry {
    pub async fn open(path: &Path, initialize: bool) -> io::Result<Self> {
        let claim = Arc::new(Claim::acquire(path, initialize)?);
        if initialize {
            let mut options = private_options();
            options.write(true).create_new(true);
            claim.directory.open_with("identity.sqlite", &options)?;
        }
        let file = claim
            .directory
            .open_with("identity.sqlite", private_options().read(true))?
            .into_std();
        validate_private(&file.metadata()?, false)?;
        let filename = database_path(path, &claim.directory);
        let options = SqliteConnectOptions::new()
            .filename(filename)
            .create_if_missing(false)
            .journal_mode(sqlx::sqlite::SqliteJournalMode::Delete)
            .synchronous(sqlx::sqlite::SqliteSynchronous::Full)
            .busy_timeout(std::time::Duration::ZERO)
            .disable_statement_logging();
        let mut connection = SqliteConnection::connect_with(&options)
            .await
            .map_err(database_error)?;
        sqlx::query("PRAGMA max_page_count = 16384")
            .execute(&mut connection)
            .await
            .map_err(database_error)?;
        sqlx::query("PRAGMA journal_size_limit = 67108864")
            .execute(&mut connection)
            .await
            .map_err(database_error)?;
        if !initialize {
            let row = sqlx::query("SELECT version FROM registry WHERE singleton = 1")
                .fetch_one(&mut connection)
                .await
                .map_err(database_error)?;
            if row.get::<i64, _>(0) != 1 {
                return Err(io::Error::other("Unsupported folder state version"));
            }
        }
        Ok(Self {
            connection,
            claim,
            initialized: !initialize,
        })
    }

    pub async fn commit(mut self, catalog: &mut Catalog) -> io::Result<()> {
        let mut transaction = self.connection.begin().await.map_err(database_error)?;
        if !self.initialized {
            sqlx::query("CREATE TABLE registry (singleton INTEGER PRIMARY KEY CHECK (singleton = 1), version INTEGER NOT NULL, identity TEXT NOT NULL, generation INTEGER NOT NULL)")
                .execute(&mut *transaction).await.map_err(database_error)?;
            sqlx::query("CREATE TABLE recordings (id TEXT PRIMARY KEY, checksum TEXT NOT NULL, detail TEXT NOT NULL)")
                .execute(&mut *transaction).await.map_err(database_error)?;
            sqlx::query("CREATE INDEX recording_checksum ON recordings(checksum)")
                .execute(&mut *transaction)
                .await
                .map_err(database_error)?;
            sqlx::query("INSERT INTO registry VALUES (1, 1, ?, 0)")
                .bind(uuid::Uuid::new_v4().to_string())
                .execute(&mut *transaction)
                .await
                .map_err(database_error)?;
        }
        let count: i64 = sqlx::query_scalar("SELECT count(*) FROM recordings")
            .fetch_one(&mut *transaction)
            .await
            .map_err(database_error)?;
        if count > 10000 || catalog.tracks.len() > 10000 {
            return Err(io::Error::other("Folder state recording limit exceeded"));
        }
        for track in catalog.tracks.values() {
            let detail = serde_json::to_string(&track.recording()).map_err(io::Error::other)?;
            sqlx::query("INSERT INTO recordings (id, checksum, detail) VALUES (?, ?, ?) ON CONFLICT (id) DO UPDATE SET detail = excluded.detail")
                .bind(&track.id)
                .bind(&track.id)
                .bind(detail)
                .execute(&mut *transaction).await.map_err(database_error)?;
        }
        let count: i64 = sqlx::query_scalar("SELECT count(*) FROM recordings")
            .fetch_one(&mut *transaction)
            .await
            .map_err(database_error)?;
        if count > 10000 {
            return Err(io::Error::other("Folder state recording limit exceeded"));
        }
        let updated = sqlx::query("UPDATE registry SET generation = generation + 1 WHERE singleton = 1 AND generation < 9223372036854775807")
            .execute(&mut *transaction).await.map_err(database_error)?;
        if updated.rows_affected() != 1 {
            return Err(io::Error::other("Folder state generation limit exceeded"));
        }
        let row = sqlx::query("SELECT identity, generation FROM registry WHERE singleton = 1")
            .fetch_one(&mut *transaction)
            .await
            .map_err(database_error)?;
        let identity: String = row.get(0);
        let generation: i64 = row.get(1);
        transaction.commit().await.map_err(database_error)?;
        self.connection.close().await.map_err(database_error)?;
        catalog.fingerprint = format!("{identity}-{generation}");
        catalog.claim = Some(self.claim);
        tracing::info!(
            event = "folder_registry_committed",
            outcome = "committed",
            generation
        );
        Ok(())
    }
}

impl Claim {
    fn acquire(path: &Path, initialize: bool) -> io::Result<Self> {
        if initialize {
            let mut builder = std::fs::DirBuilder::new();
            #[cfg(unix)]
            {
                use std::os::unix::fs::DirBuilderExt;
                builder.mode(0o700);
            }
            match builder.create(path) {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                Err(error) => return Err(error),
            }
        }
        let directory = crate::scan::open_root(path)?;
        validate_private(&directory.try_clone()?.into_std_file().metadata()?, true)?;
        let mut options = private_options();
        options.read(true).write(true).create(true);
        let lock = directory.open_with("owner.lock", &options)?.into_std();
        validate_private(&lock.metadata()?, false)?;
        lock.try_lock()
            .map_err(|_| io::Error::other("Folder state is already owned"))?;
        Ok(Self {
            directory,
            _lock: lock,
        })
    }
}

fn private_options() -> OpenOptions {
    let mut options = OpenOptions::new();
    options.follow(FollowSymlinks::No);
    #[cfg(unix)]
    {
        use cap_std::fs::OpenOptionsExt;
        options.mode(0o600).custom_flags(libc::O_NONBLOCK);
    }
    options
}

fn validate_private(metadata: &std::fs::Metadata, directory: bool) -> io::Result<()> {
    if metadata.is_dir() != directory || (!directory && !metadata.is_file()) {
        return Err(io::Error::other("Invalid folder state entry"));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let required = if directory { 0o700 } else { 0o600 };
        if metadata.uid() != unsafe { libc::geteuid() } || metadata.mode() & 0o777 != required {
            return Err(io::Error::other("Folder state must be private and owned"));
        }
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn database_path(_: &Path, directory: &Dir) -> std::path::PathBuf {
    use std::os::fd::AsRawFd;
    format!("/proc/self/fd/{}/identity.sqlite", directory.as_raw_fd()).into()
}
#[cfg(not(target_os = "linux"))]
fn database_path(path: &Path, _: &Dir) -> std::path::PathBuf {
    path.join("identity.sqlite")
}

fn database_error(_: sqlx::Error) -> io::Error {
    io::Error::other("Folder state operation failed")
}
