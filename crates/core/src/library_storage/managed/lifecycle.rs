use super::{ManagedFiles, blocking, sync_directory, unavailable};
use crate::{Error, Result};
use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex, Weak,
        atomic::{AtomicBool, Ordering},
    },
    time::Instant,
};
use tokio::sync::{Mutex as AsyncMutex, OwnedMutexGuard, OwnedSemaphorePermit, Semaphore};
use uuid::Uuid;

struct Attempt {
    revoked: AtomicBool,
    writer: Arc<AsyncMutex<()>>,
    inspection: Arc<Semaphore>,
}

#[derive(Clone)]
pub(crate) struct ManagedAttempt {
    pub(super) file_id: Uuid,
    pub(super) attempt_id: Uuid,
    inner: Arc<Attempt>,
}

pub(super) struct Writing {
    _writer: OwnedMutexGuard<()>,
    _inspection: Option<OwnedSemaphorePermit>,
    attempt: ManagedAttempt,
}
pub(super) type Writer = Arc<Writing>;
type Registry = HashMap<(Uuid, Uuid), Weak<Attempt>>;

#[derive(Clone, Default)]
pub(super) struct Writers(Arc<Mutex<Registry>>);

impl ManagedFiles {
    pub(crate) async fn settle(&self) -> Result<()> {
        let started = Instant::now();
        if let Some(root) = &self.root {
            let _idle = root.slots.acquire().await.map_err(|_| stopped())?;
        }
        tracing::info!(
            event = "download_storage_settled",
            outcome = "retired",
            elapsed_ms = started.elapsed().as_millis() as u64,
            "Managed download storage operations retired"
        );
        Ok(())
    }
}

impl ManagedFiles {
    pub(super) async fn sweep_staging(&self) -> Result<()> {
        let Some(root) = self.root.clone() else {
            return Ok(());
        };
        let started = Instant::now();
        let slots = root.slots.clone();
        let removed = blocking::run(slots, move |_| {
            let mut removed = 0_u64;
            for entry in root.directory.entries().map_err(|_| unavailable())? {
                let entry = entry.map_err(|_| unavailable())?;
                let is_staging = entry.file_name().to_str().is_some_and(is_staging_name);
                if !is_staging || !entry.file_type().map_err(|_| unavailable())?.is_file() {
                    continue;
                }
                match root.directory.remove_file(entry.file_name()) {
                    Ok(()) => removed += 1,
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                    Err(_) => return Err(unavailable()),
                }
            }
            if removed > 0 {
                sync_directory(&root)?;
            }
            Ok(removed)
        })
        .await?;
        tracing::info!(
            event = "download_staging_swept",
            removed,
            elapsed_ms = started.elapsed().as_millis() as u64,
            "Leftover download staging files removed"
        );
        Ok(())
    }
}

fn is_staging_name(name: &str) -> bool {
    let Some((file, attempt)) = name
        .strip_suffix(".part")
        .and_then(|stem| stem.split_once('.'))
    else {
        return false;
    };
    [file, attempt]
        .iter()
        .all(|part| Uuid::parse_str(part).is_ok_and(|id| id.to_string() == *part))
}

impl Writers {
    pub fn active(&self, file_id: Uuid, attempt_id: Uuid) -> Result<bool> {
        Ok(self
            .0
            .lock()
            .map_err(|_| stopped())?
            .get(&(file_id, attempt_id))
            .is_some_and(|attempt| attempt.strong_count() > 0))
    }

    pub fn reserve(&self, file_id: Uuid, attempt_id: Uuid) -> Result<ManagedAttempt> {
        let mut registry = self.0.lock().map_err(|_| stopped())?;
        registry.retain(|_, attempt| attempt.strong_count() > 0);
        if registry
            .get(&(file_id, attempt_id))
            .and_then(Weak::upgrade)
            .is_some()
        {
            return Err(stopped());
        }
        let inner = Arc::new(Attempt {
            revoked: AtomicBool::new(false),
            writer: Arc::new(AsyncMutex::new(())),
            inspection: Arc::new(Semaphore::new(1)),
        });
        registry.insert((file_id, attempt_id), Arc::downgrade(&inner));
        Ok(ManagedAttempt {
            file_id,
            attempt_id,
            inner,
        })
    }

    pub fn revoke(&self, file_id: Uuid, attempt_id: Uuid) -> Result<ManagedAttempt> {
        let mut registry = self.0.lock().map_err(|_| stopped())?;
        registry.retain(|_, attempt| attempt.strong_count() > 0);
        let inner = registry
            .get(&(file_id, attempt_id))
            .and_then(Weak::upgrade)
            .unwrap_or_else(|| {
                let inner = Arc::new(Attempt {
                    revoked: AtomicBool::new(true),
                    writer: Arc::new(AsyncMutex::new(())),
                    inspection: Arc::new(Semaphore::new(1)),
                });
                registry.insert((file_id, attempt_id), Arc::downgrade(&inner));
                inner
            });
        inner.revoked.store(true, Ordering::SeqCst);
        Ok(ManagedAttempt {
            file_id,
            attempt_id,
            inner,
        })
    }
}

impl ManagedAttempt {
    pub(crate) async fn retire(&self) -> Result<()> {
        let started = Instant::now();
        drop(self.cleanup().await?);
        tracing::debug!(file_id = %self.file_id, attempt_id = %self.attempt_id, outcome = "retired", elapsed_ms = started.elapsed().as_millis() as u64, "Download attempt operations retired");
        Ok(())
    }

    pub(super) async fn begin(&self) -> Result<Writer> {
        self.check()?;
        let writer = self.inner.writer.clone().lock_owned().await;
        self.check()?;
        Ok(Arc::new(Writing {
            _writer: writer,
            _inspection: None,
            attempt: self.clone(),
        }))
    }

    pub(super) async fn cleanup(&self) -> Result<Writer> {
        self.inner.revoked.store(true, Ordering::SeqCst);
        let writer = self.inner.writer.clone().lock_owned().await;
        let inspection = self
            .inner
            .inspection
            .clone()
            .acquire_owned()
            .await
            .map_err(|_| stopped())?;
        Ok(Arc::new(Writing {
            _writer: writer,
            _inspection: Some(inspection),
            attempt: self.clone(),
        }))
    }

    fn check(&self) -> Result<()> {
        if self.inner.revoked.load(Ordering::SeqCst) {
            Err(Error::Conflict(
                "This download attempt was withdrawn".into(),
            ))
        } else {
            Ok(())
        }
    }
}

impl Writing {
    pub(super) fn check(&self) -> Result<()> {
        self.attempt.check()
    }
    pub(super) fn inspection(self: &Arc<Self>) -> crate::library_storage::inspection::Scope {
        crate::library_storage::inspection::Scope::new(
            self.attempt.inner.inspection.clone(),
            self.clone(),
        )
    }
}

fn stopped() -> Error {
    Error::Internal("Download storage stopped".into())
}
