use hito_core::Services;
use std::{net::SocketAddr, sync::Arc, time::Duration};
use tokio::{
    net::{TcpListener, TcpStream},
    sync::{Notify, oneshot},
    task::JoinHandle,
};
use tracing::{Subscriber, instrument::WithSubscriber};
use tracing_subscriber::{Layer, layer::Context, prelude::*};

#[derive(Clone, Default)]
pub struct Handoff(Arc<Notify>);

impl<S: Subscriber> Layer<S> for Handoff {
    fn on_event(&self, event: &tracing::Event<'_>, _: Context<'_, S>) {
        struct Marker(bool);
        impl tracing::field::Visit for Marker {
            fn record_debug(&mut self, _: &tracing::field::Field, _: &dyn std::fmt::Debug) {}
            fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
                if field.name() == "event"
                    && matches!(
                        value,
                        "http_source_resolution_waiting_cleanup"
                            | "source_resolution_handoff_waiting"
                    )
                {
                    self.0 = true;
                }
            }
        }
        let mut marker = Marker(false);
        event.record(&mut marker);
        if marker.0 {
            self.0.notify_one();
        }
    }
}

impl Handoff {
    pub async fn entered(&self) {
        tokio::time::timeout(Duration::from_secs(3), self.0.notified())
            .await
            .expect("the first foreground request must reach its held cancellation handoff");
    }
}

pub struct Host {
    pub address: SocketAddr,
    stop: Option<oneshot::Sender<()>>,
    task: Option<JoinHandle<bool>>,
}

impl Host {
    pub async fn start(services: Services, handoff: Handoff) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let config = hito_server::Config {
            origin: format!("http://{address}"),
            ..Default::default()
        };
        let (stop, stopped) = oneshot::channel();
        hito_testkit::logs::anchor_callsite_interest();
        let subscriber = tracing_subscriber::registry().with(handoff);
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
