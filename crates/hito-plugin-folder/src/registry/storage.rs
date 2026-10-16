use cap_fs_ext::{FollowSymlinks, OpenOptionsFollowExt};
use cap_std::fs::{Dir, OpenOptions};
use sqlx::SqliteConnection;
use std::{fs::File, io, path::Path};

const MAX_STATE_BYTES: u64 = 64 * 1024 * 1024;

pub(super) struct PageBudget {
    size: u64,
    pages: i64,
}

impl PageBudget {
    pub(super) async fn configure(connection: &mut SqliteConnection) -> io::Result<Self> {
        let size: i64 = sqlx::query_scalar("PRAGMA page_size")
            .fetch_one(&mut *connection)
            .await
            .map_err(super::database_error)?;
        let size =
            u64::try_from(size).map_err(|_| io::Error::other("Invalid folder state page size"))?;
        if !(512..=65536).contains(&size) || !size.is_power_of_two() {
            return Err(io::Error::other("Invalid folder state page size"));
        }
        let budget = Self {
            size,
            pages: (MAX_STATE_BYTES / size) as i64,
        };
        budget.check(connection).await?;
        let limit: i64 = sqlx::query_scalar(&format!("PRAGMA max_page_count = {}", budget.pages))
            .fetch_one(&mut *connection)
            .await
            .map_err(super::database_error)?;
        if limit != budget.pages {
            return Err(io::Error::other("Folder state page limit was not applied"));
        }
        sqlx::query("PRAGMA journal_size_limit = 67108864")
            .execute(&mut *connection)
            .await
            .map_err(super::database_error)?;
        Ok(budget)
    }
    pub(super) async fn check(&self, connection: &mut SqliteConnection) -> io::Result<()> {
        let size: i64 = sqlx::query_scalar("PRAGMA page_size")
            .fetch_one(&mut *connection)
            .await
            .map_err(super::database_error)?;
        let pages: i64 = sqlx::query_scalar("PRAGMA page_count")
            .fetch_one(&mut *connection)
            .await
            .map_err(super::database_error)?;
        if size != self.size as i64 || !(0..=self.pages).contains(&pages) {
            return Err(io::Error::other("Folder state page limit exceeded"));
        }
        Ok(())
    }
}

pub(crate) struct Claim {
    directory: Dir,
    path: std::path::PathBuf,
    _lock: File,
}

impl Claim {
    pub(super) fn database_path(&self) -> std::path::PathBuf {
        database_path(&self.path, &self.directory)
    }
    pub(super) fn check_database(&self) -> io::Result<()> {
        let file = self
            .directory
            .open_with("identity.sqlite", private_options().read(true))?
            .into_std();
        let metadata = file.metadata()?;
        validate_private(&metadata, false)?;
        if metadata.len() > MAX_STATE_BYTES {
            return Err(io::Error::other("Folder state file limit exceeded"));
        }
        Ok(())
    }
    pub fn snapshot(&self) -> io::Result<File> {
        let file = tempfile::tempfile_in(state_path(&self.path, &self.directory))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            file.set_permissions(std::fs::Permissions::from_mode(0o600))?;
        }
        validate_private(&file.metadata()?, false)?;
        tracing::trace!(event = "folder_snapshot_created", outcome = "private");
        Ok(file)
    }
    pub(super) fn acquire(path: &Path, initialize: bool) -> io::Result<Self> {
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
        let claim = Self {
            directory,
            path: path.to_owned(),
            _lock: lock,
        };
        if initialize {
            let mut options = private_options();
            options.write(true).create_new(true);
            claim.directory.open_with("identity.sqlite", &options)?;
        }
        claim.check_database()?;
        Ok(claim)
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
fn database_path(path: &Path, directory: &Dir) -> std::path::PathBuf {
    state_path(path, directory).join("identity.sqlite")
}
#[cfg(target_os = "linux")]
fn state_path(_: &Path, directory: &Dir) -> std::path::PathBuf {
    use std::os::fd::AsRawFd;
    format!("/proc/self/fd/{}", directory.as_raw_fd()).into()
}
#[cfg(not(target_os = "linux"))]
fn database_path(path: &Path, _: &Dir) -> std::path::PathBuf {
    path.join("identity.sqlite")
}
#[cfg(not(target_os = "linux"))]
fn state_path(path: &Path, _: &Dir) -> std::path::PathBuf {
    path.to_owned()
}
