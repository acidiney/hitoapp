mod ancestry;
mod files;
mod ownership;

use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Duration,
};

use sqlx::{
    ConnectOptions, Connection, TransactionManager,
    sqlite::{
        SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteTransactionManager,
    },
};
use tokio::sync::{oneshot, watch};
use tracing::{Instrument, instrument::WithSubscriber};

use crate::Error;
use ownership::WorkerTracker;

static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./migrations");

pub(crate) struct Storage {
    pub(crate) pool: sqlx::SqlitePool,
    stop: Mutex<Option<oneshot::Sender<()>>>,
    finished: watch::Receiver<bool>,
}

impl Storage {
    pub(crate) async fn open(path: PathBuf) -> Result<Self, Error> {
        tokio::spawn(Self::initialize(path).with_current_subscriber())
            .await
            .map_err(|_| Error::StorageUnavailable)?
    }

    async fn initialize(path: PathBuf) -> Result<Self, Error> {
        let owner = Arc::new(
            tokio::task::spawn_blocking(move || files::StateOwner::acquire(&path))
                .await
                .map_err(|_| Error::StateUnavailable)??,
        );
        let tracker = Arc::new(WorkerTracker::new());
        let worker_owner = Arc::downgrade(&owner);
        let worker_tracker = tracker.clone();
        let options = SqliteConnectOptions::new()
            .filename(owner.database_path())
            .create_if_missing(false)
            .foreign_keys(true)
            .journal_mode(SqliteJournalMode::Wal)
            .busy_timeout(Duration::from_secs(2))
            .worker_guard(move || worker_tracker.lease(&worker_owner))
            .disable_statement_logging();
        let connected = SqlitePoolOptions::new()
            .max_connections(1)
            .acquire_timeout(Duration::from_secs(3))
            .after_release(|connection, _| {
                Box::pin(async move {
                    connection.ping().await?;
                    Ok(SqliteTransactionManager::get_transaction_depth(connection) == 0)
                })
            })
            .connect_with(options)
            .await;
        let pool = match connected {
            Ok(pool) => pool,
            Err(_) => {
                tracker.close_gate();
                tracker.wait_empty().await;
                drop(owner);
                return Err(Error::StorageUnavailable);
            }
        };
        if MIGRATOR.run(&pool).await.is_err() {
            tracker.close_gate();
            close_pool(&pool).await;
            tracker.wait_empty().await;
            drop(owner);
            return Err(Error::StorageUnavailable);
        }
        let (stop, receiver) = oneshot::channel();
        let (closed, finished) = watch::channel(false);
        let lifecycle_pool = pool.clone();
        let lifecycle = async move {
            let _ = receiver.await;
            let started = std::time::Instant::now();
            tracker.close_gate();
            close_pool(&lifecycle_pool).await;
            tracker.wait_empty().await;
            drop(owner);
            tracing::info!(
                event = "community_state_closed",
                outcome = "joined",
                duration_ms = started.elapsed().as_millis() as u64
            );
            let _ = closed.send(true);
        };
        tokio::spawn(
            lifecycle
                .instrument(tracing::debug_span!("community_state_lifecycle"))
                .with_current_subscriber(),
        );
        tracing::info!(event = "community_state_opened", outcome = "ready");
        Ok(Self {
            pool,
            stop: Mutex::new(Some(stop)),
            finished,
        })
    }

    pub(crate) async fn close(&self) {
        let sender = self
            .stop
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .take();
        if let Some(sender) = sender {
            let _ = sender.send(());
        }
        let mut finished = self.finished.clone();
        let _ = finished.wait_for(|closed| *closed).await;
    }
}

async fn close_pool(pool: &sqlx::SqlitePool) {
    loop {
        pool.close().await;
        if pool.size() == 0 {
            return;
        }
    }
}
