use axum::{Router, body::to_bytes};
use reqwest::{Method, StatusCode};
use serde_json::Value;
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

pub async fn json(
    client: &reqwest::Client,
    server: &Server,
    method: Method,
    path: &str,
    token: Option<&str>,
    body: Option<Value>,
) -> (StatusCode, Vec<u8>) {
    let mut request = client
        .request(method, server.base.join(path).expect("plugin path"))
        .header("x-hito-plugin-protocol", "1")
        .header("cookie", super::COOKIE);
    if let Some(token) = token {
        request = request.bearer_auth(token);
    }
    if let Some(body) = body {
        request = request.json(&body);
    }
    let response = request
        .send()
        .await
        .expect("owned plugin HTTP request must finish");
    let status = response.status();
    assert_eq!(response.headers()["cache-control"], "private, no-store");
    let mut bytes = Vec::new();
    let mut response = response;
    while let Some(chunk) = response.chunk().await.expect("owned JSON body must finish") {
        assert!(
            chunk.len() <= 65_536_usize.saturating_sub(bytes.len()),
            "JSON exceeded its bound"
        );
        bytes.extend_from_slice(&chunk);
    }
    (status, bytes)
}

pub async fn capture_request(
    request: axum::extract::Request,
) -> (axum::http::request::Parts, Vec<u8>) {
    let (parts, body) = request.into_parts();
    assert!(
        parts.uri.to_string().len() <= 4096,
        "owned request target exceeded its bound"
    );
    assert!(
        parts.headers.len() <= 32,
        "owned request header count exceeded its bound"
    );
    assert!(
        parts
            .headers
            .iter()
            .map(|(name, value)| name.as_str().len() + value.as_bytes().len())
            .sum::<usize>()
            <= 8192,
        "owned request headers exceeded their byte bound"
    );
    let body = to_bytes(body, 4096)
        .await
        .expect("owned request body must remain bounded");
    (parts, body.to_vec())
}
