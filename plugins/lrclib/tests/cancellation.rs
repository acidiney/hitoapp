mod support;

use axum::{body::Body, http::Request};
use hito_plugin_lrclib::{Config, router};
use serde_json::json;
use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use support::{TOKEN, record};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    sync::oneshot,
    task::JoinHandle,
};
use tower::ServiceExt;
use url::Url;

fn request() -> Request<Body> {
    Request::post("/v2/lyrics")
        .header("authorization", format!("Bearer {TOKEN}"))
        .header("x-hito-plugin-protocol", "1")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({"recording":{"id":"41","revision":null}}).to_string(),
        ))
        .unwrap()
}

struct Held {
    url: String,
    started: Option<oneshot::Receiver<()>>,
    retired: Option<oneshot::Receiver<()>>,
    task: Option<JoinHandle<()>>,
    calls: Arc<AtomicUsize>,
}

impl Held {
    async fn start(retry: bool) -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/", listener.local_addr().unwrap());
        let (started_tx, started) = oneshot::channel();
        let (retired_tx, retired) = oneshot::channel();
        let calls = Arc::new(AtomicUsize::new(0));
        let count = calls.clone();
        let task = tokio::spawn(async move {
            let (mut connection, _) = listener.accept().await.unwrap();
            read_request(&mut connection).await;
            count.fetch_add(1, Ordering::SeqCst);
            connection.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 10000\r\nConnection: close\r\n\r\n{\"id\":41").await.unwrap();
            started_tx.send(()).unwrap();
            let mut byte = [0u8; 1];
            match connection.read(&mut byte).await {
                Ok(0) => {}
                Err(error) if error.kind() == std::io::ErrorKind::ConnectionReset => {}
                other => panic!("owned held response did not retire: {other:?}"),
            }
            retired_tx.send(()).unwrap();
            if retry {
                let (mut connection, _) = listener.accept().await.unwrap();
                read_request(&mut connection).await;
                count.fetch_add(1, Ordering::SeqCst);
                let body = record(41).to_string();
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                connection.write_all(response.as_bytes()).await.unwrap();
                connection.shutdown().await.unwrap();
            }
        });
        Self {
            url,
            started: Some(started),
            retired: Some(retired),
            task: Some(task),
            calls,
        }
    }

    async fn close(mut self) {
        tokio::time::timeout(Duration::from_secs(3), self.task.take().unwrap())
            .await
            .unwrap()
            .unwrap();
    }
}

impl Drop for Held {
    fn drop(&mut self) {
        if let Some(task) = &self.task {
            task.abort();
        }
    }
}

async fn read_request(connection: &mut tokio::net::TcpStream) {
    let mut bytes = Vec::new();
    let mut byte = [0u8; 1];
    while !bytes.ends_with(b"\r\n\r\n") {
        assert!(bytes.len() < 4096);
        connection.read_exact(&mut byte).await.unwrap();
        bytes.push(byte[0]);
    }
    assert!(bytes.starts_with(b"GET /api/get/41 HTTP/1.1\r\n"));
    assert!(
        !String::from_utf8(bytes)
            .unwrap()
            .to_ascii_lowercase()
            .contains("authorization:")
    );
}

#[tokio::test]
async fn cancelling_a_held_body_releases_sequential_admission_before_a_real_retry() {
    let mut provider = Held::start(true).await;
    let app = router(
        Config::new(TOKEN)
            .unwrap()
            .with_endpoint(Url::parse(&provider.url).unwrap())
            .unwrap(),
    )
    .unwrap();
    {
        let pending = app.clone().oneshot(request());
        tokio::pin!(pending);
        tokio::select! {
            _ = &mut pending => panic!("held body completed before cancellation"),
            started = provider.started.take().unwrap() => started.unwrap(),
        }
        assert_eq!(app.clone().oneshot(request()).await.unwrap().status(), 503);
        assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
    }
    tokio::time::timeout(Duration::from_secs(2), provider.retired.take().unwrap())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(app.oneshot(request()).await.unwrap().status(), 200);
    assert_eq!(provider.calls.load(Ordering::SeqCst), 2);
    provider.close().await;
}

#[tokio::test]
async fn the_total_deadline_includes_the_entire_upstream_response_body() {
    let mut provider = Held::start(false).await;
    let app = router(
        Config::new(TOKEN)
            .unwrap()
            .with_endpoint(Url::parse(&provider.url).unwrap())
            .unwrap(),
    )
    .unwrap();
    let started = tokio::time::Instant::now();
    let response = tokio::time::timeout(Duration::from_secs(8), app.oneshot(request()))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(response.status(), 503);
    assert!(started.elapsed() >= Duration::from_secs(5));
    tokio::time::timeout(Duration::from_secs(2), provider.retired.take().unwrap())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
    provider.close().await;
}
