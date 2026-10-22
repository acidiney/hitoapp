use axum::Router;
use std::time::Duration;
use tokio::{sync::oneshot, task::JoinHandle};
use tracing::instrument::WithSubscriber;
use url::Url;

pub struct Server {
    pub base: Url,
    stop: Option<oneshot::Sender<()>>,
    task: Option<JoinHandle<std::io::Result<()>>>,
}

impl Server {
    pub async fn start(router: Router, dispatch: tracing::Dispatch) -> Self {
        let router = router.layer(axum::middleware::from_fn(
            move |request: axum::extract::Request, next: axum::middleware::Next| {
                let dispatch = dispatch.clone();
                async move { next.run(request).with_subscriber(dispatch).await }
            },
        ));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("owned plugin listener must bind");
        let base = format!("http://{}/", listener.local_addr().expect("plugin address"))
            .parse()
            .expect("owned plugin origin must parse");
        let (stop, stopped) = oneshot::channel();
        let task = tokio::spawn(async move {
            axum::serve(listener, router)
                .with_graceful_shutdown(async {
                    let _ = stopped.await;
                })
                .await
        });
        Self {
            base,
            stop: Some(stop),
            task: Some(task),
        }
    }

    pub async fn close(mut self) -> Result<(), &'static str> {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        if let Some(mut task) = self.task.take() {
            match tokio::time::timeout(Duration::from_secs(3), &mut task).await {
                Ok(Ok(Ok(()))) => {}
                Ok(_) => return Err("owned plugin server failed"),
                Err(_) => {
                    task.abort();
                    let _ = task.await;
                    return Err("owned plugin server failed to join shutdown");
                }
            }
        }
        Ok(())
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }
}
