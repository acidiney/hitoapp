use hito_core::Services;
use std::{
    net::SocketAddr,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tokio::{
    net::{TcpListener, TcpStream},
    sync::{Notify, oneshot},
    task::JoinHandle,
};
use tracing::{Subscriber, instrument::WithSubscriber};
use tracing_subscriber::{Layer, layer::Context, prelude::*};
use uuid::Uuid;

#[derive(Default)]
struct State {
    writing: Notify,
    shutdown: Notify,
    forced: Notify,
    draining: Notify,
    stopped: Notify,
    committed: AtomicBool,
    finished: AtomicBool,
    stopped_before_commit: AtomicBool,
    resolution: Mutex<Option<Uuid>>,
}

#[derive(Clone, Default)]
pub struct Signals(Arc<State>);

#[derive(Clone, Copy)]
enum Event {
    Writing,
    Committed,
    Shutdown,
    Forced,
    Draining,
    Stopped,
}

#[derive(Default)]
struct Marker {
    event: Option<Event>,
    resolution: Option<Uuid>,
}

impl tracing::field::Visit for Marker {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        if field.name() == "resolution_id" {
            self.resolution = Uuid::parse_str(format!("{value:?}").trim_matches('"')).ok();
        }
    }

    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        match field.name() {
            "resolution_id" => self.resolution = Uuid::parse_str(value).ok(),
            "event" => {
                self.event = match value {
                    "source_resolution_write_started" => Some(Event::Writing),
                    "source_resolution_committed" => Some(Event::Committed),
                    "shutdown_requested" => Some(Event::Shutdown),
                    "connections_cancelled" => Some(Event::Forced),
                    "http_source_resolutions_draining" => Some(Event::Draining),
                    "server_stopped" => Some(Event::Stopped),
                    _ => None,
                }
            }
            _ => {}
        }
    }
}

impl<S: Subscriber> Layer<S> for Signals {
    fn on_event(&self, event: &tracing::Event<'_>, _: Context<'_, S>) {
        let mut marker = Marker::default();
        event.record(&mut marker);
        match marker.event {
            Some(Event::Writing) => {
                *self.0.resolution.lock().unwrap() = marker.resolution;
                self.0.writing.notify_one();
            }
            Some(Event::Committed) => self.0.committed.store(true, Ordering::SeqCst),
            Some(Event::Shutdown) => self.0.shutdown.notify_one(),
            Some(Event::Forced) => self.0.forced.notify_one(),
            Some(Event::Draining) => self.0.draining.notify_one(),
            Some(Event::Stopped) => {
                self.0
                    .stopped_before_commit
                    .store(!self.0.committed.load(Ordering::SeqCst), Ordering::SeqCst);
                self.0.finished.store(true, Ordering::SeqCst);
                self.0.stopped.notify_one();
            }
            None => {}
        }
    }
}

impl Signals {
    pub async fn writing(&self) -> Uuid {
        tokio::time::timeout(Duration::from_secs(3), self.0.writing.notified())
            .await
            .expect("the real HTTP resolver must enter publication while SQLite is held");
        self.0
            .resolution
            .lock()
            .unwrap()
            .expect("publication must expose its safe resolution identity")
    }

    pub async fn shutdown(&self) {
        tokio::time::timeout(Duration::from_secs(3), self.0.shutdown.notified())
            .await
            .expect("the owned host must observe its requested shutdown");
    }

    pub async fn forced(&self) {
        tokio::time::timeout(Duration::from_secs(6), self.0.forced.notified())
            .await
            .expect("the retained client must reach the existing five-second connection abort");
    }

    pub async fn assert_draining_before_release(&self) {
        assert!(
            !self.0.finished.load(Ordering::SeqCst),
            "the host must not stop before publication's writer lock is released"
        );
        tokio::time::timeout(Duration::from_secs(3), async {
            tokio::select! {
                biased;
                _ = self.0.stopped.notified() => panic!("the host stopped before publication's writer lock was released"),
                _ = self.0.draining.notified() => {},
            }
        }).await.expect("the host must join its publication after connection ownership ends");
        assert!(
            tokio::time::timeout(Duration::from_millis(50), self.0.stopped.notified())
                .await
                .is_err(),
            "a blocked publication must keep the host owned throughout the pending check"
        );
        assert!(
            !self.0.committed.load(Ordering::SeqCst),
            "publication cannot commit while the external writer is held"
        );
        assert!(
            !self.0.finished.load(Ordering::SeqCst),
            "server_stopped must follow publication settlement"
        );
    }

    pub fn assert_committed_and_stopped(&self) {
        assert!(
            !self.0.stopped_before_commit.load(Ordering::SeqCst),
            "server_stopped must never precede the accepted publication commit"
        );
        assert!(
            self.0.committed.load(Ordering::SeqCst),
            "accepted publication must commit before its host finishes"
        );
        assert!(
            self.0.finished.load(Ordering::SeqCst),
            "the joined host must report server_stopped"
        );
    }
}

pub struct Host {
    pub address: SocketAddr,
    stop: Option<oneshot::Sender<()>>,
    task: Option<JoinHandle<bool>>,
}

impl Host {
    pub async fn start(services: Services, signals: Signals) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let config = hito_server::Config {
            origin: format!("http://{address}"),
            ..Default::default()
        };
        let (stop, stopped) = oneshot::channel();
        let subscriber = tracing_subscriber::registry().with(signals);
        let task = tokio::spawn(
            async move {
                hito_server::serve(
                    listener,
                    hito_server::router_with_config(services.clone(), config),
                    services,
                    async {
                        let _ = stopped.await;
                    },
                )
                .await
                .is_ok()
            }
            .with_subscriber(subscriber),
        );
        Self {
            address,
            stop: Some(stop),
            task: Some(task),
        }
    }

    pub fn request_shutdown(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
    }

    pub fn is_finished(&self) -> bool {
        self.task.as_ref().is_none_or(JoinHandle::is_finished)
    }

    pub async fn join(&mut self) -> bool {
        let Some(mut task) = self.task.take() else {
            return true;
        };
        match tokio::time::timeout(Duration::from_secs(7), &mut task).await {
            Ok(result) => {
                matches!(result, Ok(true)) && TcpStream::connect(self.address).await.is_err()
            }
            Err(_) => {
                task.abort();
                let _ = task.await;
                false
            }
        }
    }
}

impl Drop for Host {
    fn drop(&mut self) {
        self.request_shutdown();
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }
}
