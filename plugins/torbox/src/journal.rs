mod admission;
mod commands;
mod connection;
mod files;
mod header;
mod receipt_set;
mod records;
mod transactions;

pub(crate) use commands::{Handle, Step};
pub(crate) use records::Target;

use crate::{InitError, StateConfig, StateError, catalog::Catalog};
use hito_debrid_torbox::TorBoxClient;
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Instant,
};
use tokio::sync::{mpsc, oneshot, watch};
use tracing::{Instrument, instrument::WithSubscriber};
use uuid::Uuid;

pub(crate) struct Journal {
    handle: Handle,
    stop: Option<oneshot::Sender<()>>,
    finished: watch::Receiver<Option<Result<(), StateError>>>,
}

impl Journal {
    pub(crate) fn validate_catalog(catalog: &Catalog) -> Result<(), InitError> {
        for index in 0..catalog.len() {
            records::binding_bytes(&catalog.entry(index).binding)
                .map_err(|_| InitError::InvalidCatalog)?;
        }
        Ok(())
    }

    pub(crate) async fn open(
        config: StateConfig,
        client: &TorBoxClient,
        catalog: &Catalog,
    ) -> Result<Self, StateError> {
        let witness = header::catalog_witness(catalog);
        let client = client.clone();
        let targets = (0..catalog.len())
            .map(|index| Target::from(catalog.entry(index)))
            .collect();
        let gate = Arc::new(AtomicBool::new(false));
        let lifecycle_gate = gate.clone();
        let (commands, receiver) = mpsc::channel(16);
        let handle = Handle { gate, commands };
        let (stop, stopped) = oneshot::channel();
        let (ready, initialized) = oneshot::channel();
        let (closed, finished) = watch::channel(None);
        tokio::spawn(
            async move {
                let mut log = Log::new("journal_open", "torbox_journal_open_finished");
                match connection::initialize(config, client, witness, targets)
                    .instrument(log.span.clone())
                    .await
                {
                    Ok(mut state) => {
                        lifecycle_gate.store(true, Ordering::Release);
                        log.outcome = if ready.send(Ok(())).is_ok() {
                            "ready"
                        } else {
                            "cancelled"
                        };
                        drop(log);
                        let served =
                            commands::serve(&mut state, receiver, stopped, &lifecycle_gate).await;
                        lifecycle_gate.store(false, Ordering::Release);
                        let closed_state = state.close().await;
                        let result = served.and(closed_state);
                        let _ = closed.send(Some(result));
                    }
                    Err(error) => {
                        log.outcome = "rejected";
                        let _ = ready.send(Err(error));
                        let _ = closed.send(Some(Err(error)));
                    }
                }
            }
            .with_current_subscriber(),
        );
        let journal = Self {
            handle,
            stop: Some(stop),
            finished,
        };
        initialized.await.map_err(|_| StateError::Unavailable)??;
        Ok(journal)
    }

    pub(crate) fn gate(&self) -> Arc<AtomicBool> {
        self.handle.gate.clone()
    }

    pub(crate) fn handle(&self) -> Handle {
        self.handle.clone()
    }

    pub(crate) async fn close(mut self) -> Result<(), StateError> {
        self.stop();
        self.finished
            .wait_for(|result| result.is_some())
            .await
            .map_err(|_| StateError::Unavailable)?;
        self.finished
            .borrow()
            .as_ref()
            .copied()
            .ok_or(StateError::Unavailable)?
    }

    fn stop(&mut self) {
        self.handle.gate.store(false, Ordering::Release);
        drop(self.stop.take());
    }
}

impl Drop for Journal {
    fn drop(&mut self) {
        self.stop();
    }
}

fn database_error(error: sqlx::Error) -> StateError {
    if let Some(sqlite_code) = error
        .as_database_error()
        .and_then(|error| error.code())
        .and_then(|code| code.parse::<u64>().ok())
    {
        tracing::debug!(
            event = "torbox_journal_database_rejected",
            outcome = "database_error",
            sqlite_code
        );
    }
    StateError::Unavailable
}

struct Log {
    started: Instant,
    dispatch: tracing::Dispatch,
    span: tracing::Span,
    event: &'static str,
    outcome: &'static str,
    count: u64,
}

impl Log {
    fn new(operation: &'static str, event: &'static str) -> Self {
        Self {
            started: Instant::now(),
            dispatch: tracing::dispatcher::get_default(Clone::clone),
            span: tracing::debug_span!("torbox_journal", request_id = %Uuid::new_v4(), operation),
            event,
            outcome: "cancelled",
            count: 0,
        }
    }
}

impl Drop for Log {
    fn drop(&mut self) {
        let _dispatch = tracing::dispatcher::set_default(&self.dispatch);
        let span = std::mem::replace(&mut self.span, tracing::Span::none());
        let _entered = span.enter();
        tracing::debug!(
            event = self.event,
            outcome = self.outcome,
            count = self.count,
            elapsed_ms = self.started.elapsed().as_millis() as u64
        );
    }
}
