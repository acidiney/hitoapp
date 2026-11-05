use std::{
    fmt,
    sync::{
        Arc, Condvar, Mutex, OnceLock,
        atomic::{AtomicBool, Ordering},
    },
};
use tokio::sync::Notify;
use tracing::{
    Dispatch, Event, Subscriber,
    field::{Field, Visit},
    span::Id,
};
use tracing_subscriber::{Layer, layer::Context, registry::LookupSpan};

static ANCHORS: OnceLock<[Dispatch; 2]> = OnceLock::new();

pub fn consult_every_dispatcher_for_callsites() {
    ANCHORS.get_or_init(|| {
        [
            Dispatch::new(tracing_subscriber::registry()),
            Dispatch::new(tracing_subscriber::registry()),
        ]
    });
}

#[derive(Clone, Default)]
pub struct AdmissionGate {
    armed: Arc<AtomicBool>,
    entered: Arc<Notify>,
    released: Arc<(Mutex<bool>, Condvar)>,
    deleting: Arc<Notify>,
    admitting: Arc<Notify>,
    order: Arc<Mutex<Vec<&'static str>>>,
}
pub struct Release(AdmissionGate);
impl Drop for Release {
    fn drop(&mut self) {
        self.0.release();
    }
}
impl AdmissionGate {
    pub fn arm(&self) {
        self.armed.store(true, Ordering::Release);
    }
    pub fn guard(&self) -> Release {
        Release(self.clone())
    }
    pub async fn entered(&self) {
        self.entered.notified().await;
    }
    pub async fn deleting(&self) {
        self.deleting.notified().await;
    }
    pub async fn admitting(&self) {
        self.admitting.notified().await;
    }
    pub fn order(&self) -> Vec<&'static str> {
        self.order.lock().unwrap().clone()
    }
    pub fn release(&self) {
        *self.released.0.lock().unwrap() = true;
        self.released.1.notify_all();
    }
}
#[derive(Default)]
struct Workflow {
    admission: bool,
    published: bool,
    deleted: bool,
}
impl Visit for Workflow {
    fn record_str(&mut self, field: &Field, value: &str) {
        if field.name() == "event" {
            self.admission |= value == "audiobook_admission_ready";
            self.published |= value == "player_command_accepted";
        }
        if field.name() == "message" {
            self.deleted |= value == "Sync batch committed";
            self.admission |= value == "Audiobook chapter admission completed";
        }
    }
    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        if field.name() == "message" {
            self.record_str(field, &format!("{value:?}"));
        }
    }
}
impl<S: Subscriber + for<'span> LookupSpan<'span>> Layer<S> for AdmissionGate {
    fn on_enter(&self, id: &Id, context: Context<'_, S>) {
        let name = context.span(id).unwrap().metadata().name();
        if name == "apply" {
            self.deleting.notify_one();
        }
        if name == "admit_audiobook_playback" {
            self.admitting.notify_one();
        }
    }
    fn on_event(&self, event: &Event<'_>, _: Context<'_, S>) {
        let mut workflow = Workflow::default();
        event.record(&mut workflow);
        if workflow.deleted {
            self.order.lock().unwrap().push("deleted");
        }
        if workflow.published && !self.armed.load(Ordering::Acquire) {
            self.order.lock().unwrap().push("published");
        }
        if workflow.admission && self.armed.swap(false, Ordering::AcqRel) {
            self.entered.notify_one();
            tokio::task::block_in_place(|| {
                let mut released = self.released.0.lock().unwrap();
                while !*released {
                    released = self.released.1.wait(released).unwrap();
                }
            });
        }
    }
}
