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
}

#[derive(Clone)]
pub struct HeaderBarrier(Arc<State>);

impl Default for HeaderBarrier {
    fn default() -> Self {
        Self(Arc::new(State {
            armed: AtomicBool::new(false),
            entered: Semaphore::new(0),
            released: Mutex::new(false),
            release: Condvar::new(),
            timed_out: AtomicBool::new(false),
        }))
    }
}

impl HeaderBarrier {
    pub fn arm(&self) -> HeaderRelease {
        assert!(!self.0.armed.swap(true, Ordering::SeqCst));
        HeaderRelease(self.clone())
    }

    pub async fn entered(&self) {
        self.0.entered.acquire().await.unwrap().forget();
    }

    pub fn release(&self) {
        *self
            .0
            .released
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = true;
        self.0.release.notify_all();
    }

    pub fn timed_out(&self) -> bool {
        self.0.timed_out.load(Ordering::SeqCst)
    }
}

pub struct HeaderRelease(HeaderBarrier);

impl Drop for HeaderRelease {
    fn drop(&mut self) {
        self.0.release();
    }
}

impl<S: Subscriber> Layer<S> for HeaderBarrier {
    fn on_event(&self, event: &Event<'_>, _context: Context<'_, S>) {
        if event.metadata().target() != "sqlx::query" || !self.0.armed.load(Ordering::SeqCst) {
            return;
        }
        let mut statement = HeaderStatement(false);
        event.record(&mut statement);
        if !statement.0
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
    }
}

struct HeaderStatement(bool);

impl HeaderStatement {
    fn inspect(&mut self, value: &str) {
        self.0 = value.contains("SELECT id, name, kind FROM playlists")
            && value.contains("WHERE id = ? AND user_id = ? AND deleted = 0");
    }
}

impl Visit for HeaderStatement {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        if field.name() == "db.statement" {
            self.inspect(&format!("{value:?}"));
        }
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        if field.name() == "db.statement" {
            self.inspect(value);
        }
    }
}
