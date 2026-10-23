use crate::{catalog::Catalog, registry::Registry, scan};
use std::{
    io,
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Instant,
};

#[derive(Clone, Default)]
pub(crate) struct Cancellation(Arc<AtomicBool>);
impl Cancellation {
    pub fn check(&self) -> io::Result<()> {
        if self.0.load(Ordering::Acquire) {
            Err(io::Error::new(
                io::ErrorKind::ConnectionAborted,
                "Folder startup cancelled",
            ))
        } else {
            Ok(())
        }
    }
}

struct Caller {
    cancellation: Cancellation,
    completed: bool,
}
impl Drop for Caller {
    fn drop(&mut self) {
        if !self.completed {
            self.cancellation.0.store(true, Ordering::Release);
        }
    }
}

pub(crate) async fn load(music: &Path, state: &Path, initialize: bool) -> io::Result<Catalog> {
    let cancellation = Cancellation::default();
    let mut caller = Caller {
        cancellation: cancellation.clone(),
        completed: false,
    };
    let music = music.to_owned();
    let state = state.to_owned();
    let runtime = tokio::runtime::Handle::current();
    let dispatch = tracing::dispatcher::get_default(Clone::clone);
    let span =
        tracing::info_span!("folder_startup", workflow_id = %uuid::Uuid::new_v4(), initialize);
    let started = Instant::now();
    let result = tokio::task::spawn_blocking(move || {
        tracing::dispatcher::with_default(&dispatch, || {
            let _entered = span.enter();
            let result = runtime.block_on(run(&music, &state, initialize, &cancellation));
            let outcome = if cancellation.0.load(Ordering::Acquire) {
                "cancelled"
            } else if result.is_ok() {
                "completed"
            } else {
                "failed"
            };
            tracing::debug!(
                event = "folder_startup_finished",
                outcome,
                elapsed_ms = started.elapsed().as_millis() as u64
            );
            result
        })
    })
    .await
    .map_err(|_| io::Error::other("Folder startup worker failed"));
    caller.completed = true;
    result?
}

async fn run(
    music: &Path,
    state: &Path,
    initialize: bool,
    cancellation: &Cancellation,
) -> io::Result<Catalog> {
    cancellation.check()?;
    let registry = Registry::open(state, initialize, cancellation).await?;
    let mut catalog = match scan::scan_owned(music, registry.claim(), cancellation) {
        Ok(catalog) => catalog,
        Err(error) => {
            registry.close().await?;
            return Err(error);
        }
    };
    registry.commit(&mut catalog, cancellation).await?;
    cancellation.check()?;
    Ok(catalog)
}
