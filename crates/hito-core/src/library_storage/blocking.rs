use crate::{Error, Result};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::Duration;
use tokio::sync::Semaphore;

struct CancelOnDrop(Arc<AtomicBool>);
impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Relaxed);
    }
}

pub(super) async fn run<T: Send + 'static>(
    slots: Arc<Semaphore>,
    work: impl FnOnce(&AtomicBool) -> Result<T> + Send + 'static,
) -> Result<T> {
    let cancelled = Arc::new(AtomicBool::new(false));
    let _cancel = CancelOnDrop(cancelled.clone());
    let span = tracing::Span::current();
    let operation = async move {
        let permit = slots.acquire_owned().await.map_err(|_| stopped())?;
        tokio::task::spawn_blocking(move || {
            let _permit = permit;
            check(&cancelled)?;
            span.in_scope(|| work(&cancelled))
        })
        .await
        .map_err(|_| stopped())?
    };
    tokio::time::timeout(Duration::from_secs(5), operation)
        .await
        .map_err(|_| stopped())?
}

pub(super) fn check(cancelled: &AtomicBool) -> Result<()> {
    if cancelled.load(Ordering::Relaxed) {
        Err(stopped())
    } else {
        Ok(())
    }
}

fn stopped() -> Error {
    Error::Internal("Storage operation stopped. Refresh and try again".into())
}
