use crate::{CoreConfig, Error, Result};
use sqlx::sqlite::SqliteConnectOptions;
use std::{
    path::Path,
    sync::{Arc, Condvar, Mutex, Weak},
    time::{Duration, Instant},
};
use tokio::sync::{Semaphore, watch};

#[derive(Clone, Copy, Default)]
struct Resources {
    active: usize,
    closing: bool,
    retiring: bool,
    deadline_exceeded: bool,
    opened: bool,
    published: bool,
    failed: bool,
}

struct Workers {
    resources: Resources,
    opening: Weak<AsyncPause>,
    retirement: Weak<Pause>,
}

struct Tracker {
    state: Mutex<Workers>,
    changed: watch::Sender<Resources>,
}

struct Inner {
    directory: Arc<tempfile::TempDir>,
    tracker: Arc<Tracker>,
    pool: Mutex<Option<sqlx::SqlitePool>>,
}

impl Drop for Inner {
    fn drop(&mut self) {
        let mut state = self
            .tracker
            .state
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        state.resources.closing = true;
        self.tracker.changed.send_replace(state.resources);
    }
}

/// Test-only ownership of one Core open and its worker-local SQLite resource leases.
/// Retirement does not join OS threads or thread-local destructors.
#[derive(Clone)]
pub struct DatabaseOwner {
    inner: Arc<Inner>,
}

#[derive(Clone)]
pub struct DatabaseObservation {
    tracker: Arc<Tracker>,
}

#[derive(Clone, Copy, Debug)]
pub struct Retirement {
    pub deadline_exceeded: bool,
    pub resources_retired: bool,
}

pub struct PoolReservation {
    connections: Vec<sqlx::pool::PoolConnection<sqlx::Sqlite>>,
    _owner: DatabaseOwner,
}

impl PoolReservation {
    pub async fn release(mut self) {
        for connection in &mut self.connections {
            connection.return_to_pool().await;
        }
    }
}

impl DatabaseOwner {
    pub fn retain(directory: Arc<tempfile::TempDir>) -> Self {
        let (changed, _) = watch::channel(Resources::default());
        Self {
            inner: Arc::new(Inner {
                directory,
                tracker: Arc::new(Tracker {
                    state: Mutex::new(Workers {
                        resources: Resources::default(),
                        opening: Weak::new(),
                        retirement: Weak::new(),
                    }),
                    changed,
                }),
                pool: Mutex::new(None),
            }),
        }
    }

    pub fn attach(&self, config: &mut CoreConfig) {
        config.database_owner = Some(self.clone());
    }

    pub fn observe(&self) -> DatabaseObservation {
        DatabaseObservation {
            tracker: self.inner.tracker.clone(),
        }
    }

    pub async fn reserve_all(&self) -> Result<PoolReservation> {
        let pool = {
            let state = self
                .inner
                .tracker
                .state
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            if state.resources.closing {
                return Err(Error::Conflict("The owned test database is closing".into()));
            }
            self.inner
                .pool
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .clone()
                .ok_or_else(|| Error::Conflict("The owned test database is not open".into()))?
        };
        let mut connections = Vec::new();
        for _ in 0..pool.options().get_max_connections() {
            connections.push(pool.acquire().await?);
        }
        Ok(PoolReservation {
            connections,
            _owner: self.clone(),
        })
    }

    pub fn hold_opening(&self, gate: &OpeningGate) {
        self.inner
            .tracker
            .state
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .opening = Arc::downgrade(&gate.inner);
    }

    pub fn hold_retirement(&self, gate: &RetirementGate) {
        self.inner
            .tracker
            .state
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .retirement = Arc::downgrade(&gate.inner);
    }

    pub(crate) fn options(
        &self,
        path: &Path,
        options: SqliteConnectOptions,
    ) -> Result<SqliteConnectOptions> {
        validate_path(self.inner.directory.path(), path)?;
        let mut state = self
            .inner
            .tracker
            .state
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if state.resources.closing || state.resources.opened {
            return Err(Error::Conflict(
                "The owned test database cannot open again".into(),
            ));
        }
        state.resources.opened = true;
        self.inner.tracker.changed.send_replace(state.resources);
        drop(state);
        let tracker = self.inner.tracker.clone();
        let directory = Arc::downgrade(&self.inner.directory);
        Ok(options.worker_guard(move || tracker.lease(&directory)))
    }

    pub(crate) fn register(&self, pool: &sqlx::SqlitePool) -> Result<()> {
        let mut state = self
            .inner
            .tracker
            .state
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if state.resources.closing || state.resources.published {
            return Err(Error::Conflict("The owned test database is closing".into()));
        }
        *self
            .inner
            .pool
            .lock()
            .unwrap_or_else(|error| error.into_inner()) = Some(pool.clone());
        state.resources.published = true;
        self.inner.tracker.changed.send_replace(state.resources);
        Ok(())
    }

    /// Call after stopping Core callers and dropping their Services handles.
    /// A deadline is reported as failure; it never releases storage before lease retirement.
    #[tracing::instrument(level = "debug", skip_all)]
    pub async fn retire(&self, deadline: Duration) -> Retirement {
        let started = Instant::now();
        let pool = {
            let mut state = self
                .inner
                .tracker
                .state
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            state.resources.closing = true;
            state.resources.retiring = true;
            self.inner.tracker.changed.send_replace(state.resources);
            self.inner
                .pool
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .take()
        };
        let draining = async {
            if let Some(pool) = pool {
                pool.close().await;
            }
            // SQLite close drops external connection references before the worker guard retires.
            let mut changed = self.inner.tracker.changed.subscribe();
            let _ = changed
                .wait_for(|resources| resources.active == 0 || resources.failed)
                .await;
        };
        tokio::pin!(draining);
        let deadline_exceeded = tokio::time::timeout(deadline, &mut draining).await.is_err();
        if deadline_exceeded {
            {
                let mut state = self
                    .inner
                    .tracker
                    .state
                    .lock()
                    .unwrap_or_else(|error| error.into_inner());
                state.resources.deadline_exceeded = true;
                self.inner.tracker.changed.send_replace(state.resources);
            }
            draining.await;
        }
        let resources = *self.inner.tracker.changed.borrow();
        let deadline_exceeded = deadline_exceeded || resources.deadline_exceeded;
        let resources_retired = resources.closing && resources.active == 0 && !resources.failed;
        tracing::debug!(
            event = "owned_database_retired",
            resources_retired,
            deadline_exceeded,
            elapsed_ms = started.elapsed().as_millis() as u64
        );
        Retirement {
            deadline_exceeded,
            resources_retired,
        }
    }
}

impl DatabaseObservation {
    pub(crate) async fn after_connect(&self) {
        let pause = self
            .tracker
            .state
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .opening
            .upgrade();
        if let Some(pause) = pause {
            pause.entered.add_permits(1);
            let _ = pause.release.acquire().await;
        }
    }

    pub fn active_resources(&self) -> usize {
        self.tracker.changed.borrow().active
    }
    pub fn pool_published(&self) -> bool {
        self.tracker.changed.borrow().published
    }

    pub async fn retirement_started(&self) {
        let mut changed = self.tracker.changed.subscribe();
        let _ = changed.wait_for(|resources| resources.retiring).await;
    }
    pub async fn retirement_deadline(&self) {
        let mut changed = self.tracker.changed.subscribe();
        let _ = changed
            .wait_for(|resources| resources.deadline_exceeded)
            .await;
    }
}

struct Lease {
    directory: Option<Arc<tempfile::TempDir>>,
    tracker: Arc<Tracker>,
}

impl Tracker {
    fn lease(
        self: &Arc<Self>,
        directory: &Weak<tempfile::TempDir>,
    ) -> std::result::Result<Lease, sqlx::Error> {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        if state.resources.closing {
            return Err(sqlx::Error::PoolClosed);
        }
        let directory = directory.upgrade().ok_or(sqlx::Error::PoolClosed)?;
        state.resources.active = state
            .resources
            .active
            .checked_add(1)
            .ok_or(sqlx::Error::PoolClosed)?;
        self.changed.send_replace(state.resources);
        Ok(Lease {
            directory: Some(directory),
            tracker: self.clone(),
        })
    }
}

impl Drop for Lease {
    fn drop(&mut self) {
        let pause = self
            .tracker
            .state
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .retirement
            .upgrade();
        if let Some(pause) = pause {
            pause.entered.add_permits(1);
            let mut released = pause
                .released
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            while !*released {
                released = pause
                    .changed
                    .wait(released)
                    .unwrap_or_else(|error| error.into_inner());
            }
        }
        drop(self.directory.take());
        let mut state = self
            .tracker
            .state
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if let Some(active) = state.resources.active.checked_sub(1) {
            state.resources.active = active;
        } else {
            state.resources.failed = true;
        }
        self.tracker.changed.send_replace(state.resources);
    }
}

struct AsyncPause {
    entered: Semaphore,
    release: Semaphore,
}
struct Pause {
    entered: Semaphore,
    released: Mutex<bool>,
    changed: Condvar,
}

pub struct OpeningGate {
    inner: Arc<AsyncPause>,
}
pub struct RetirementGate {
    inner: Arc<Pause>,
}

impl Default for OpeningGate {
    fn default() -> Self {
        Self::new()
    }
}
impl Default for RetirementGate {
    fn default() -> Self {
        Self::new()
    }
}

impl OpeningGate {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(AsyncPause {
                entered: Semaphore::new(0),
                release: Semaphore::new(0),
            }),
        }
    }
    pub async fn entered(&self) {
        if let Ok(permit) = self.inner.entered.acquire().await {
            permit.forget();
        }
    }
    pub fn release(&self) {
        self.inner.release.close();
    }
}
impl Drop for OpeningGate {
    fn drop(&mut self) {
        self.release();
    }
}

impl RetirementGate {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Pause {
                entered: Semaphore::new(0),
                released: Mutex::new(false),
                changed: Condvar::new(),
            }),
        }
    }
    pub async fn entered(&self) {
        if let Ok(permit) = self.inner.entered.acquire().await {
            permit.forget();
        }
    }
    pub fn release(&self) {
        *self
            .inner
            .released
            .lock()
            .unwrap_or_else(|error| error.into_inner()) = true;
        self.inner.changed.notify_all();
    }
}
impl Drop for RetirementGate {
    fn drop(&mut self) {
        self.release();
    }
}

fn validate_path(directory: &Path, path: &Path) -> Result<()> {
    let owned = directory.canonicalize().ok();
    if owned.is_none()
        || path.parent().and_then(|path| path.canonicalize().ok()) != owned
        || path.file_name().is_none()
    {
        return Err(Error::Invalid(
            "Use a database file directly inside the owned test directory".into(),
        ));
    }
    for suffix in ["", "-wal", "-shm", "-journal"] {
        let mut file = path.as_os_str().to_os_string();
        file.push(suffix);
        match std::fs::symlink_metadata(&file) {
            Ok(metadata) if !metadata.is_file() || metadata.file_type().is_symlink() => {
                return Err(Error::Invalid(
                    "The owned test database must use private regular files".into(),
                ));
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => {
                return Err(Error::Invalid(
                    "The owned test database files could not be checked".into(),
                ));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
