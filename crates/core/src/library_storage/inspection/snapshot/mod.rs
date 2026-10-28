mod file;
mod id3;

use super::ownership::Scope;
use crate::{Error, Result, library_storage::root::Root};
use std::{
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
use tempfile::TempDir;
use tokio::sync::{Semaphore, oneshot};

#[derive(PartialEq)]
pub(super) struct Snapshot {
    pub path: PathBuf,
    pub checksum: String,
    pub size: i64,
}

pub(super) struct MediaSnapshot {
    pub path: PathBuf,
    pub directory: Arc<TempDir>,
}

pub(super) struct Prepared {
    pub file: Snapshot,
    pub media: Option<MediaSnapshot>,
}

#[derive(Clone)]
pub(super) struct Reader {
    slots: Arc<Semaphore>,
    retirement: Option<Scope>,
}

enum Mode {
    Checksum,
    Prepare(Option<String>),
}

struct CancelOnDrop(Arc<AtomicBool>);

impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Relaxed);
    }
}

struct Control {
    cancelled: Arc<AtomicBool>,
    started: Instant,
    timeout: Duration,
}

impl Control {
    fn check(&self) -> Result<()> {
        if self.cancelled.load(Ordering::Relaxed) || self.started.elapsed() >= self.timeout {
            return Err(stopped());
        }
        Ok(())
    }
}

impl Reader {
    pub(super) fn new() -> Self {
        Self {
            slots: Arc::new(Semaphore::new(2)),
            retirement: None,
        }
    }

    pub(super) fn scoped(&self, retirement: Scope) -> Self {
        Self {
            slots: self.slots.clone(),
            retirement: Some(retirement),
        }
    }

    pub(super) async fn settle(&self) -> Result<()> {
        let _idle = self.slots.acquire_many(2).await.map_err(|_| stopped())?;
        Ok(())
    }

    pub(super) async fn checksum(
        &self,
        root: &Root,
        path: &Path,
        timeout: Duration,
    ) -> Result<Snapshot> {
        Ok(self.read(root, path, Mode::Checksum, timeout).await?.file)
    }

    pub(super) async fn prepare(
        &self,
        root: &Root,
        path: &Path,
        cached_checksum: Option<&str>,
        timeout: Duration,
    ) -> Result<Prepared> {
        self.read(
            root,
            path,
            Mode::Prepare(cached_checksum.map(str::to_owned)),
            timeout,
        )
        .await
    }

    async fn read(
        &self,
        root: &Root,
        path: &Path,
        mode: Mode,
        timeout: Duration,
    ) -> Result<Prepared> {
        let control = Control {
            cancelled: Arc::new(AtomicBool::new(false)),
            started: Instant::now(),
            timeout,
        };
        let _cancel = CancelOnDrop(control.cancelled.clone());
        let slots = self.slots.clone();
        let retirement = self.retirement.clone();
        let root = root.clone();
        let path = path.to_owned();
        let span = tracing::Span::current();
        let operation = async move {
            let permit = slots.acquire_owned().await.map_err(|_| stopped())?;
            let retirement = match retirement {
                Some(scope) => Some(scope.acquire().await?),
                None => None,
            };
            let (finished, result) = oneshot::channel();
            let _task = tokio::task::spawn_blocking(move || {
                let _permit = permit;
                let _retirement = retirement;
                let _entered = span.enter();
                let outcome = file::read(&root, &path, mode, &control);
                let _ = finished.send(outcome);
            });
            result.await.map_err(|_| unreadable())?
        };
        tokio::time::timeout(timeout, operation)
            .await
            .map_err(|_| stopped())?
    }
}

fn stopped() -> Error {
    Error::Internal("Media verification stopped".into())
}
fn unreadable() -> Error {
    Error::Internal("Media file could not be verified".into())
}
fn changed() -> Error {
    Error::Conflict("Media changed during inspection; scan again".into())
}
