mod budget;
mod file;
mod id3;

pub(in crate::library_storage::inspection) use budget::PrivateCopy;
use budget::{Budget, Reservation};

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
use tokio::sync::{Semaphore, oneshot};

#[derive(PartialEq)]
pub(super) struct Snapshot {
    pub path: PathBuf,
    pub checksum: String,
    pub size: i64,
}

pub(super) struct MediaSnapshot {
    pub path: PathBuf,
    pub directory: Arc<PrivateCopy>,
    pub recording_id: Option<String>,
    pub artists: Option<super::tagged_artists::TaggedArtists>,
}

pub(super) struct Prepared {
    pub file: Snapshot,
    pub media: Option<MediaSnapshot>,
}

const THREADS: usize = 256;

#[derive(Clone)]
pub(super) struct Reader {
    slots: Arc<Semaphore>,
    threads: Arc<Semaphore>,
    retirement: Option<Scope>,
    budget: Budget,
    temp_dir: Option<PathBuf>,
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
    temp_dir: Option<PathBuf>,
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
    pub(super) fn new(temp_budget: u64, temp_dir: Option<PathBuf>) -> Self {
        Self {
            slots: Arc::new(Semaphore::new(2)),
            threads: Arc::new(Semaphore::new(THREADS)),
            retirement: None,
            budget: Budget::new(temp_budget),
            temp_dir,
        }
    }

    pub(super) fn scoped(&self, retirement: Scope) -> Self {
        Self {
            slots: self.slots.clone(),
            threads: self.threads.clone(),
            retirement: Some(retirement),
            budget: self.budget.clone(),
            temp_dir: self.temp_dir.clone(),
        }
    }

    pub(super) async fn settle(&self) -> Result<()> {
        let _idle = self
            .threads
            .acquire_many(THREADS as u32)
            .await
            .map_err(|_| stopped())?;
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
        let reservation = match mode {
            Mode::Prepare(_) => {
                let size = self.measure(root, path, timeout).await?;
                Some(self.budget.reserve(size).await?)
            }
            Mode::Checksum => None,
        };
        let control = Control {
            cancelled: Arc::new(AtomicBool::new(false)),
            started: Instant::now(),
            timeout,
            temp_dir: self.temp_dir.clone(),
        };
        let _cancel = CancelOnDrop(control.cancelled.clone());
        let slots = self.slots.clone();
        let threads = self.threads.clone();
        let retirement = self.retirement.clone();
        let root = root.clone();
        let path = path.to_owned();
        let span = tracing::Span::current();
        let operation = async move {
            let drive = root
                .read_slots
                .clone()
                .acquire_owned()
                .await
                .map_err(|_| stopped())?;
            let _active = slots.acquire_owned().await.map_err(|_| stopped())?;
            let live = threads.acquire_owned().await.map_err(|_| stopped())?;
            let retirement = match retirement {
                Some(scope) => Some(scope.acquire().await?),
                None => None,
            };
            let (finished, result) = oneshot::channel();
            let _task = tokio::task::spawn_blocking(move || {
                let _drive = drive;
                let _live = live;
                let _retirement = retirement;
                let _entered = span.enter();
                let outcome = file::read(&root, &path, mode, &control, reservation);
                let _ = finished.send(outcome);
            });
            result.await.map_err(|_| unreadable())?
        };
        tokio::time::timeout(timeout, operation)
            .await
            .map_err(|_| stopped())?
    }

    async fn measure(&self, root: &Root, path: &Path, timeout: Duration) -> Result<u64> {
        let cancelled = Arc::new(AtomicBool::new(false));
        let _cancel = CancelOnDrop(cancelled.clone());
        let slots = self.slots.clone();
        let threads = self.threads.clone();
        let root = root.clone();
        let path = path.to_owned();
        let operation = async move {
            let drive = root
                .read_slots
                .clone()
                .acquire_owned()
                .await
                .map_err(|_| stopped())?;
            let _active = slots.acquire_owned().await.map_err(|_| stopped())?;
            let live = threads.acquire_owned().await.map_err(|_| stopped())?;
            let (finished, result) = oneshot::channel();
            let _task = tokio::task::spawn_blocking(move || {
                let _drive = drive;
                let _live = live;
                let size = if cancelled.load(Ordering::Relaxed) {
                    Err(stopped())
                } else {
                    root.open_file(&path)
                        .and_then(|file| file.metadata())
                        .map(|metadata| metadata.len())
                        .map_err(|_| unreadable())
                };
                let _ = finished.send(size);
            });
            result.await.map_err(|_| unreadable())?
        };
        tokio::time::timeout(timeout, operation)
            .await
            .map_err(|_| stopped())?
    }
}

fn copy_failure(error: &std::io::Error) -> Error {
    match error.kind() {
        std::io::ErrorKind::StorageFull | std::io::ErrorKind::QuotaExceeded => {
            Error::InsufficientStorage
        }
        _ => unreadable(),
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Error as IoError, ErrorKind};

    #[test]
    fn a_full_or_over_quota_temporary_disk_is_insufficient_storage() {
        for kind in [ErrorKind::StorageFull, ErrorKind::QuotaExceeded] {
            assert!(matches!(
                copy_failure(&IoError::from(kind)),
                Error::InsufficientStorage
            ));
        }
        assert!(matches!(
            copy_failure(&IoError::from(ErrorKind::PermissionDenied)),
            Error::Internal(_)
        ));
    }
}
