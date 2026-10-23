use crate::{catalog::Catalog, startup::Cancellation};
use futures_util::TryStreamExt;
use hito_plugin_sdk::ProviderRecording;
use sqlx::{ConnectOptions, Connection, Row, SqliteConnection, sqlite::SqliteConnectOptions};
use std::{collections::BTreeMap, io, path::Path, sync::Arc};

mod storage;
pub(crate) use storage::Claim;
use storage::PageBudget;

pub(crate) struct Registry {
    connection: SqliteConnection,
    claim: Arc<Claim>,
    initialized: bool,
    version: i64,
    budget: PageBudget,
}

impl Registry {
    pub async fn open(
        path: &Path,
        initialize: bool,
        cancellation: &Cancellation,
    ) -> io::Result<Self> {
        cancellation.check()?;
        let claim = Arc::new(Claim::acquire(path, initialize)?);
        cancellation.check()?;
        let options = SqliteConnectOptions::new()
            .filename(claim.database_path())
            .create_if_missing(false)
            .busy_timeout(std::time::Duration::ZERO)
            .disable_statement_logging();
        let mut connection = SqliteConnection::connect_with(&options)
            .await
            .map_err(database_error)?;
        let admitted = async {
            cancellation.check()?;
            sqlx::query("PRAGMA journal_mode = DELETE")
                .execute(&mut connection)
                .await
                .map_err(database_error)?;
            sqlx::query("PRAGMA synchronous = FULL")
                .execute(&mut connection)
                .await
                .map_err(database_error)?;
            let budget = PageBudget::configure(&mut connection).await?;
            let version = if !initialize {
                let row = sqlx::query("SELECT version FROM registry WHERE singleton = 1")
                    .fetch_one(&mut connection)
                    .await
                    .map_err(database_error)?;
                let version = row.get::<i64, _>(0);
                if !matches!(version, 1..=6) {
                    return Err(io::Error::other("Unsupported folder state version"));
                }
                if version == 6 {
                    let snapshot: Option<String> = sqlx::query_scalar("SELECT CASE WHEN typeof(snapshot) = 'text' AND length(CAST(snapshot AS BLOB)) = 36 THEN snapshot END FROM registry WHERE singleton = 1")
                        .fetch_one(&mut connection).await.map_err(database_error)?;
                    let snapshot =
                        snapshot.ok_or_else(|| io::Error::other("Invalid folder snapshot nonce"))?;
                    let parsed = uuid::Uuid::parse_str(&snapshot)
                        .map_err(|_| io::Error::other("Invalid folder snapshot nonce"))?;
                    if parsed.get_version() != Some(uuid::Version::Random)
                        || parsed.to_string() != snapshot
                    {
                        return Err(io::Error::other("Invalid folder snapshot nonce"));
                    }
                }
                version
            } else {
                0
            };
            cancellation.check()?;
            Ok::<_, io::Error>((budget, version))
        }
        .await;
        let (budget, version) = match admitted {
            Ok(admitted) => admitted,
            Err(error) => {
                connection.close().await.map_err(database_error)?;
                return Err(error);
            }
        };
        Ok(Self {
            connection,
            claim,
            initialized: !initialize,
            version,
            budget,
        })
    }

    pub async fn close(self) -> io::Result<()> {
        let result = self.connection.close().await.map_err(database_error);
        drop(self.claim);
        result
    }

    pub async fn commit(
        mut self,
        catalog: &mut Catalog,
        cancellation: &Cancellation,
    ) -> io::Result<()> {
        let staged = self.write_generation(catalog, cancellation).await;
        let closed = self.connection.close().await.map_err(database_error);
        let generation = staged?;
        closed?;
        self.claim.check_database()?;
        catalog.retained = generation.retained;
        catalog.fingerprint = generation.fingerprint;
        catalog.claim = Some(self.claim);
        tracing::info!(
            event = "folder_registry_committed",
            outcome = "committed",
            generation = generation.number
        );
        Ok(())
    }

    async fn write_generation(
        &mut self,
        catalog: &mut Catalog,
        cancellation: &Cancellation,
    ) -> io::Result<Generation> {
        cancellation.check()?;
        let mut transaction = self.connection.begin().await.map_err(database_error)?;
        let staged = async {
            cancellation.check()?;
            if !self.initialized {
                sqlx::query("CREATE TABLE registry (singleton INTEGER PRIMARY KEY CHECK (singleton = 1), version INTEGER NOT NULL, identity TEXT NOT NULL, generation INTEGER NOT NULL)")
                    .execute(&mut *transaction).await.map_err(database_error)?;
                sqlx::query("CREATE TABLE recordings (id TEXT PRIMARY KEY, checksum TEXT NOT NULL, detail TEXT NOT NULL)")
                    .execute(&mut *transaction).await.map_err(database_error)?;
                sqlx::query("CREATE INDEX recording_checksum ON recordings(checksum)")
                    .execute(&mut *transaction)
                    .await
                    .map_err(database_error)?;
                sqlx::query("INSERT INTO registry VALUES (1, 6, ?, 0)")
                    .bind(uuid::Uuid::new_v4().to_string())
                    .execute(&mut *transaction)
                    .await
                    .map_err(database_error)?;
            }
            if self.version < 2 {
                crate::bindings::create(&mut transaction)
                    .await
                    .map_err(database_error)?;
            }
            if self.version < 3 {
                crate::bindings::create_observations(&mut transaction)
                    .await
                    .map_err(database_error)?;
            }
            if self.version < 4 {
                crate::bindings::create_owners(&mut transaction)
                    .await
                    .map_err(database_error)?;
                sqlx::query("UPDATE registry SET version = 4 WHERE singleton = 1")
                    .execute(&mut *transaction)
                    .await
                    .map_err(database_error)?;
            }
            if self.version < 5 {
                crate::releases::state::create(&mut transaction).await?;
                sqlx::query("UPDATE registry SET version = 5 WHERE singleton = 1")
                    .execute(&mut *transaction)
                    .await
                    .map_err(database_error)?;
            }
            if self.version < 6 {
                sqlx::query("ALTER TABLE registry ADD COLUMN snapshot TEXT NOT NULL DEFAULT '00000000-0000-0000-0000-000000000000' CHECK(typeof(snapshot) = 'text' AND length(CAST(snapshot AS BLOB)) = 36)")
                    .execute(&mut *transaction).await.map_err(database_error)?;
                sqlx::query("UPDATE registry SET version = 6 WHERE singleton = 1")
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
            crate::bindings::reconcile(&mut transaction, catalog, !self.initialized).await?;
            let count: i64 = sqlx::query_scalar("SELECT count(*) FROM recordings")
                .fetch_one(&mut *transaction)
                .await
                .map_err(database_error)?;
            if count > 10000 {
                return Err(io::Error::other("Folder state recording limit exceeded"));
            }
            let retained = retained_metadata(&mut transaction).await?;
            let snapshot = uuid::Uuid::new_v4().to_string();
            let updated = sqlx::query("UPDATE registry SET generation = generation + 1, snapshot = ? WHERE singleton = 1 AND generation < 9223372036854775807")
                .bind(&snapshot).execute(&mut *transaction).await.map_err(database_error)?;
            if updated.rows_affected() != 1 {
                return Err(io::Error::other("Folder state generation limit exceeded"));
            }
            let row = sqlx::query("SELECT identity, generation FROM registry WHERE singleton = 1")
                .fetch_one(&mut *transaction)
                .await
                .map_err(database_error)?;
            let identity: String = row.get(0);
            let generation: i64 = row.get(1);
            self.budget.check(&mut transaction).await?;
            self.claim.check_database()?;
            cancellation.check()?;
            Ok::<_, io::Error>(Generation {
                retained,
                fingerprint: format!("{identity}-{generation}-{snapshot}"),
                number: generation,
            })
        }
        .await;
        match staged {
            Ok(generation) => {
                transaction.commit().await.map_err(database_error)?;
                Ok(generation)
            }
            Err(error) => {
                transaction.rollback().await.map_err(database_error)?;
                Err(error)
            }
        }
    }
    pub fn claim(&self) -> &Claim {
        &self.claim
    }
}

struct Generation {
    retained: BTreeMap<String, ProviderRecording>,
    fingerprint: String,
    number: i64,
}

async fn retained_metadata(
    connection: &mut SqliteConnection,
) -> io::Result<BTreeMap<String, ProviderRecording>> {
    let bounds = sqlx::query("SELECT count(*), coalesce(sum(length(CAST(detail AS BLOB))), 0), coalesce(max(length(CAST(detail AS BLOB))), 0), coalesce(max(length(CAST(id AS BLOB))), 0) FROM recordings")
        .fetch_one(&mut *connection).await.map_err(database_error)?;
    let count: i64 = bounds.try_get(0).map_err(database_error)?;
    let bytes: i64 = bounds.try_get(1).map_err(database_error)?;
    let maximum: i64 = bounds.try_get(2).map_err(database_error)?;
    let maximum_id: i64 = bounds.try_get(3).map_err(database_error)?;
    if count > 10000 || bytes > 32 * 1024 * 1024 || maximum > 32768 || maximum_id > 256 {
        return Err(io::Error::other("Folder retained metadata limit exceeded"));
    }
    let mut retained = BTreeMap::new();
    let mut rows =
        sqlx::query("SELECT id, detail FROM recordings ORDER BY id").fetch(&mut *connection);
    while let Some(row) = rows.try_next().await.map_err(database_error)? {
        let id: String = row.try_get(0).map_err(database_error)?;
        let detail: String = row.try_get(1).map_err(database_error)?;
        let recording: ProviderRecording = serde_json::from_str(&detail)
            .map_err(|_| io::Error::other("Invalid retained folder metadata"))?;
        recording
            .validate()
            .map_err(|_| io::Error::other("Invalid retained folder metadata"))?;
        if recording.id != id {
            return Err(io::Error::other(
                "Folder retained recording binding changed",
            ));
        }
        retained.insert(id, recording);
    }
    Ok(retained)
}

fn database_error(_: sqlx::Error) -> io::Error {
    io::Error::other("Folder state operation failed")
}
