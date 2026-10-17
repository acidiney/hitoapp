use sqlx::{Sqlite, SqlitePool, pool::PoolConnection};
use std::{
    future::Future,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
};

const INTERRUPT_WORK_LIMIT: u64 = 10_000_000;

pub(super) struct WorkMeter {
    active: Arc<AtomicBool>,
    steps: Arc<AtomicU64>,
}

impl WorkMeter {
    pub(super) async fn install(pool: &SqlitePool) -> Self {
        let meter = Self {
            active: Arc::new(AtomicBool::new(false)),
            steps: Arc::new(AtomicU64::new(0)),
        };
        let mut connections = all_connections(pool).await;
        for connection in &mut connections {
            let active = Arc::clone(&meter.active);
            let steps = Arc::clone(&meter.steps);
            connection
                .lock_handle()
                .await
                .unwrap()
                .set_progress_handler(100, move || {
                    if active.load(Ordering::Relaxed) {
                        return steps.fetch_add(100, Ordering::Relaxed) + 100
                            < INTERRUPT_WORK_LIMIT;
                    }
                    true
                });
        }
        for connection in &mut connections {
            connection.return_to_pool().await;
        }
        meter
    }

    pub(super) async fn measure<T>(&self, operation: impl Future<Output = T>) -> (T, u64) {
        self.steps.store(0, Ordering::Relaxed);
        self.active.store(true, Ordering::Relaxed);
        let active = ActiveMeasurement(&self.active);
        let result = operation.await;
        drop(active);
        (result, self.steps.load(Ordering::Relaxed))
    }

    pub(super) async fn remove(&self, pool: &SqlitePool) {
        self.active.store(false, Ordering::Relaxed);
        let mut connections = all_connections(pool).await;
        for connection in &mut connections {
            connection
                .lock_handle()
                .await
                .unwrap()
                .remove_progress_handler();
        }
        for connection in &mut connections {
            connection.return_to_pool().await;
        }
    }
}

impl Drop for WorkMeter {
    fn drop(&mut self) {
        self.active.store(false, Ordering::Relaxed);
    }
}

struct ActiveMeasurement<'a>(&'a AtomicBool);

impl Drop for ActiveMeasurement<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Relaxed);
    }
}

async fn all_connections(pool: &SqlitePool) -> Vec<PoolConnection<Sqlite>> {
    let count = pool.options().get_max_connections();
    let mut connections = Vec::new();
    for _ in 0..count {
        connections.push(pool.acquire().await.unwrap());
    }
    connections
}
