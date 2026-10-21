use super::provider;
use axum::Router;
use reqwest::{Method, StatusCode};
use serde_json::Value;
use std::time::Duration;
use tokio::{sync::oneshot, task::JoinHandle};
use tracing::instrument::WithSubscriber;
use url::Url;
use uuid::Uuid;

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
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}/", listener.local_addr().unwrap())
            .parse()
            .unwrap();
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
                Ok(_) => return Err("the owned plugin server failed"),
                Err(_) => {
                    task.abort();
                    let _ = task.await;
                    return Err("the owned plugin server did not stop in time");
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

pub struct Receipt {
    pub request_id: Uuid,
    pub operation: &'static str,
    pub status: u16,
}

pub async fn json(
    client: &reqwest::Client,
    server: &Server,
    path: &str,
    token: Option<&str>,
    body: Option<Value>,
    expected: StatusCode,
    receipts: &mut Vec<Receipt>,
) -> Vec<u8> {
    let operation = match path {
        "v1/manifest" => "manifest",
        "v1/search" => "search",
        "v1/recordings/detail" => "detail",
        "v1/sources" => "sources",
        "v1/sources/prepare" => "prepare",
        "v1/sources/preparation/status" => "preparation_status",
        _ => panic!("the owned JSON helper requires an exact public route"),
    };
    let method = if path == "v1/manifest" {
        Method::GET
    } else {
        Method::POST
    };
    let mut request = client
        .request(method, server.base.join(path).unwrap())
        .header("x-hito-plugin-protocol", "1")
        .header("cookie", provider::COOKIE);
    if let Some(token) = token {
        request = request.bearer_auth(token);
    }
    if let Some(body) = body {
        request = request.json(&body);
    }
    let response = request
        .send()
        .await
        .unwrap_or_else(|_| panic!("the owned plugin request failed"));
    assert_eq!(
        response.status(),
        expected,
        "the public plugin route returned an unexpected status"
    );
    record(&response, operation, receipts);
    let mut response = response;
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .unwrap_or_else(|_| panic!("the owned JSON response failed"))
    {
        assert!(chunk.len() <= 65_536_usize.saturating_sub(bytes.len()));
        bytes.extend_from_slice(&chunk);
    }
    provider::assert_private_wire(&bytes);
    bytes
}

pub fn record(response: &reqwest::Response, operation: &'static str, receipts: &mut Vec<Receipt>) {
    assert_eq!(response.headers()["cache-control"], "private, no-store");
    assert_eq!(response.headers()["x-content-type-options"], "nosniff");
    let request_id = Uuid::parse_str(response.headers()["x-request-id"].to_str().unwrap()).unwrap();
    assert_eq!(request_id.get_version_num(), 4);
    assert!(
        receipts
            .iter()
            .all(|receipt| receipt.request_id != request_id)
    );
    receipts.push(Receipt {
        request_id,
        operation,
        status: response.status().as_u16(),
    });
}

pub async fn audio(
    client: &reqwest::Client,
    server: &Server,
    receipts: &mut Vec<Receipt>,
) -> Vec<u8> {
    let mut response = client
        .get(
            server
                .base
                .join(&format!("v1/audio?source_id={}", provider::SOURCE))
                .unwrap(),
        )
        .header("x-hito-plugin-protocol", "1")
        .header("cookie", provider::COOKIE)
        .bearer_auth(provider::SERVICE)
        .send()
        .await
        .unwrap_or_else(|_| panic!("the owned audio relay failed"));
    assert_eq!(response.status(), StatusCode::OK);
    record(&response, "audio", receipts);
    assert_eq!(response.headers()["content-type"], "audio/wav");
    assert_eq!(
        response.content_length(),
        Some(super::wave_fixture::WAV_BYTES as u64)
    );
    for name in [
        "set-cookie",
        "content-disposition",
        "location",
        "authorization",
        "content-encoding",
    ] {
        assert!(response.headers().get(name).is_none());
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .unwrap_or_else(|_| panic!("the owned WAV body failed"))
    {
        assert!(!chunk.is_empty());
        assert!(bytes.len() + chunk.len() <= super::wave_fixture::WAV_BYTES);
        bytes.extend_from_slice(&chunk);
    }
    assert!(
        response
            .chunk()
            .await
            .unwrap_or_else(|_| panic!("the owned WAV EOF failed"))
            .is_none()
    );
    bytes
}
