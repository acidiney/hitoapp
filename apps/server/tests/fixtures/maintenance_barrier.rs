use crate::fixture::Library;
use std::{
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tokio::sync::Semaphore;
use tracing::{
    Event, Subscriber,
    field::{Field, Visit},
};
use tracing_subscriber::{Layer, layer::Context};

struct State {
    armed: AtomicBool,
    entered: Semaphore,
    released: Mutex<bool>,
    release: Condvar,
    timed_out: AtomicBool,
    completion: Mutex<Option<Completion>>,
    completed: Semaphore,
    library: Mutex<Option<Arc<Library>>>,
}

#[derive(Clone)]
pub struct MaintenanceBarrier(Arc<State>);

#[derive(Clone, Copy)]
pub struct Completion {
    pub complete: bool,
    pub close_complete: bool,
}

impl Default for MaintenanceBarrier {
    fn default() -> Self {
        Self(Arc::new(State {
            armed: AtomicBool::new(false),
            entered: Semaphore::new(0),
            released: Mutex::new(false),
            release: Condvar::new(),
            timed_out: AtomicBool::new(false),
            completion: Mutex::new(None),
            completed: Semaphore::new(0),
            library: Mutex::new(None),
        }))
    }
}

impl MaintenanceBarrier {
    pub fn arm(&self, library: Arc<Library>) -> Release {
        *self.0.library.lock().unwrap() = Some(library);
        assert!(!self.0.armed.swap(true, Ordering::SeqCst));
        Release(self.clone())
    }

    pub async fn entered(&self) {
        self.0.entered.acquire().await.unwrap().forget();
    }

    pub async fn completed(&self) -> Completion {
        self.0.completed.acquire().await.unwrap().forget();
        self.0.completion.lock().unwrap().unwrap()
    }

    pub fn finished(&self) -> bool {
        self.0.completion.lock().unwrap().is_some()
    }

    pub fn timed_out(&self) -> bool {
        self.0.timed_out.load(Ordering::SeqCst)
    }

    fn release(&self) {
        *self
            .0
            .released
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = true;
        self.0.release.notify_all();
    }
}

pub struct Release(MaintenanceBarrier);

impl Drop for Release {
    fn drop(&mut self) {
        self.0.release();
    }
}

impl<S: Subscriber> Layer<S> for MaintenanceBarrier {
    fn on_event(&self, event: &Event<'_>, _context: Context<'_, S>) {
        let mut fields = Fields::default();
        if event.metadata().target() == "sqlx::query" && self.0.armed.load(Ordering::SeqCst) {
            event.record(&mut fields);
            if !fields.history_read
                || self
                    .0
                    .armed
                    .compare_exchange(true, false, Ordering::SeqCst, Ordering::SeqCst)
                    .is_err()
            {
                return;
            }
            let released = self.0.released.lock().unwrap();
            self.0.entered.add_permits(1);
            let (released, _) = self
                .0
                .release
                .wait_timeout_while(released, Duration::from_secs(5), |released| !*released)
                .unwrap();
            if !*released {
                self.0.timed_out.store(true, Ordering::SeqCst);
            }
        } else if event.metadata().target() == "hito_core::maintenance" {
            event.record(&mut fields);
            if fields.finished {
                let completion = Completion {
                    complete: fields.complete,
                    close_complete: fields.close_complete,
                };
                *self.0.completion.lock().unwrap() = Some(completion);
                if completion.close_complete {
                    self.0.library.lock().unwrap().take();
                }
                self.0.completed.add_permits(1);
            }
        }
    }
}

#[derive(Default)]
struct Fields {
    history_read: bool,
    finished: bool,
    complete: bool,
    close_complete: bool,
}

impl Fields {
    fn inspect(&mut self, field: &Field, value: &str) {
        match field.name() {
            "db.statement" => {
                self.history_read =
                    value.contains("SELECT version FROM _sqlx_migrations ORDER BY version")
            }
            "message" => self.finished = value.contains("Database maintenance connection finished"),
            "outcome" => self.complete = value == "complete",
            _ => {}
        }
    }
}

impl Visit for Fields {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        self.inspect(field, &format!("{value:?}"));
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        self.inspect(field, value);
    }

    fn record_bool(&mut self, field: &Field, value: bool) {
        if field.name() == "close_complete" {
            self.close_complete = value;
        }
    }
}
