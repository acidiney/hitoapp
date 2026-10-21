use super::{
    Log, database_error,
    files::Files,
    header,
    records::{self, Phase, Target},
};
use crate::{StateConfig, StateError, StateMode};
use hito_debrid_torbox::{CandidateKey, TorBoxClient};
use sqlx::{ConnectOptions, Connection, Row, SqliteConnection, sqlite::SqliteConnectOptions};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex, Weak},
    time::{Duration, Instant},
};
use tokio::sync::oneshot;
use uuid::Uuid;
use zeroize::Zeroizing;

pub(super) struct OpenState {
    pub(super) connection: SqliteConnection,
    files: Arc<Files>,
    worker_done: oneshot::Receiver<()>,
    pub(super) key: Zeroizing<[u8; 32]>,
    pub(super) candidate_key: CandidateKey,
    pub(super) client: TorBoxClient,
    pub(super) authority: [u8; 32],
    pub(super) state_id: Uuid,
    pub(super) header: header::Header,
    pub(super) targets: Vec<Target>,
    pub(super) leases: HashMap<Uuid, Weak<()>>,
    pub(super) retry_guards: HashMap<Uuid, Instant>,
}

struct WorkerOwner {
    files: Option<Arc<Files>>,
    done: Option<oneshot::Sender<()>>,
}

impl OpenState {
    pub(super) async fn boundary(&mut self) -> Result<(), StateError> {
        checkpoint(&mut self.connection).await?;
        let files = self.files.clone();
        tokio::task::spawn_blocking(move || files.check().and_then(|()| files.sync_directory()))
            .await
            .map_err(|_| StateError::Unavailable)?
    }

    async fn validate_rows(&mut self) -> Result<(), StateError> {
        let records = records::all(
            &mut self.connection,
            &self.key,
            self.state_id,
            &self.authority,
        )
        .await?;
        let now = records::now_ms()?;
        for record in records {
            if record.phase == Phase::Ready {
                self.client
                    .restore_candidate(
                        &self.candidate_key,
                        record.context(self.state_id),
                        &record.candidate[..record.candidate_length],
                    )
                    .map_err(|_| StateError::InvalidState)?;
            }
            if matches!(record.phase, Phase::Preparing | Phase::Blocked) {
                let remaining = u64::try_from(record.retry_at_ms.saturating_sub(now).max(5000))
                    .map_err(|_| StateError::InvalidState)?;
                let deadline = Instant::now()
                    .checked_add(Duration::from_millis(remaining))
                    .ok_or(StateError::InvalidState)?;
                self.retry_guards.insert(record.operation, deadline);
            }
        }
        Ok(())
    }

    pub(super) async fn close(self) -> Result<(), StateError> {
        let mut log = Log::new("journal_close", "torbox_journal_close_finished");
        let Self {
            connection,
            files,
            worker_done,
            ..
        } = self;
        let result = connection.close().await.map_err(database_error);
        let _ = worker_done.await;
        let checked = files.check().and_then(|()| files.sync_directory());
        drop(files);
        let result = result.and(checked);
        log.outcome = if result.is_ok() {
            "joined"
        } else {
            "unavailable"
        };
        result
    }
}

impl Drop for WorkerOwner {
    fn drop(&mut self) {
        drop(self.files.take());
        if let Some(done) = self.done.take() {
            let _ = done.send(());
        }
    }
}

pub(super) async fn initialize(
    config: StateConfig,
    client: TorBoxClient,
    witness: [u8; 32],
    targets: Vec<Target>,
) -> Result<OpenState, StateError> {
    let mode = config.mode;
    let (files, raw_key) = tokio::task::spawn_blocking(move || Files::acquire(&config))
        .await
        .map_err(|_| StateError::Unavailable)??;
    let files = Arc::new(files);
    let candidate_key =
        CandidateKey::new(*raw_key).map_err(|_| StateError::InvalidConfiguration)?;
    let authority = client
        .candidate_authority(&candidate_key)
        .map_err(|_| StateError::Unavailable)?;
    let authority = *authority.as_bytes();
    let key = header::integrity_key(&raw_key)?;
    drop(raw_key);
    let (worker_finished, worker_done) = oneshot::channel();
    let completion = Arc::new(Mutex::new(Some(worker_finished)));
    let worker_completion = completion.clone();
    let worker_files = files.clone();
    let options = SqliteConnectOptions::new()
        .filename(files.database_path())
        .create_if_missing(false)
        .busy_timeout(Duration::ZERO)
        .disable_statement_logging()
        .worker_guard(move || {
            let done = worker_completion
                .lock()
                .map_err(|_| sqlx::Error::Protocol("Private state worker is unavailable".into()))?
                .take();
            Ok(WorkerOwner {
                files: Some(worker_files.clone()),
                done,
            })
        });
    let connected = SqliteConnection::connect_with(&options).await;
    drop(options);
    drop(completion);
    let mut connection = match connected {
        Ok(connection) => connection,
        Err(error) => {
            let _ = worker_done.await;
            return Err(database_error(error));
        }
    };
    let admitted = async {
        let existing = if matches!(mode, StateMode::OpenExisting) {
            Some(header::verify_header(&mut connection, &key, &authority).await?)
        } else {
            None
        };
        configure(&mut connection).await?;
        checkpoint(&mut connection).await?;
        let header = match existing {
            Some(header) => header,
            None => header::create_header(&mut connection, &key, &authority, &witness).await?,
        };
        checkpoint(&mut connection).await?;
        files.check()?;
        files.sync_directory()?;
        Ok::<_, StateError>(header)
    }
    .await;
    let header = match admitted {
        Ok(header) => header,
        Err(error) => {
            let _ = connection.close().await;
            let _ = worker_done.await;
            drop(files);
            return Err(error);
        }
    };
    let mut state = OpenState {
        connection,
        files,
        worker_done,
        key,
        candidate_key,
        client,
        authority,
        state_id: header.state_id,
        header,
        targets,
        leases: HashMap::new(),
        retry_guards: HashMap::new(),
    };
    if let Err(error) = state.validate_rows().await {
        let _ = state.close().await;
        return Err(error);
    }
    Ok(state)
}

async fn configure(connection: &mut SqliteConnection) -> Result<(), StateError> {
    for statement in [
        "PRAGMA page_size = 4096",
        "PRAGMA synchronous = FULL",
        "PRAGMA cache_spill = OFF",
        "PRAGMA temp_store = MEMORY",
        "PRAGMA wal_autocheckpoint = 0",
    ] {
        sqlx::query(statement)
            .execute(&mut *connection)
            .await
            .map_err(database_error)?;
    }
    let mode: String = sqlx::query_scalar("PRAGMA journal_mode = WAL")
        .fetch_one(&mut *connection)
        .await
        .map_err(database_error)?;
    let pages: i64 = sqlx::query_scalar("PRAGMA max_page_count = 4096")
        .fetch_one(&mut *connection)
        .await
        .map_err(database_error)?;
    if mode != "wal" || pages != 4096 {
        return Err(StateError::InvalidState);
    }
    for (statement, expected) in [
        ("PRAGMA page_size", 4096),
        ("PRAGMA synchronous", 2),
        ("PRAGMA cache_spill", 0),
        ("PRAGMA temp_store", 2),
        ("PRAGMA wal_autocheckpoint", 0),
    ] {
        let actual: i64 = sqlx::query_scalar(statement)
            .fetch_one(&mut *connection)
            .await
            .map_err(database_error)?;
        if actual != expected {
            return Err(StateError::InvalidState);
        }
    }
    Ok(())
}

async fn checkpoint(connection: &mut SqliteConnection) -> Result<(), StateError> {
    let row = sqlx::query("PRAGMA wal_checkpoint(TRUNCATE)")
        .fetch_one(connection)
        .await
        .map_err(database_error)?;
    let busy: i64 = row.try_get(0).map_err(database_error)?;
    let remaining: i64 = row.try_get(1).map_err(database_error)?;
    if busy != 0 || remaining != 0 {
        return Err(StateError::Unavailable);
    }
    Ok(())
}
