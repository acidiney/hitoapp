mod support;
use axum::{body::Body, http::Request};
use hito_plugin_folder::Folder;
use hito_testkit::logs::LogCapture;
use http_body_util::BodyExt;
use tower::ServiceExt;
use tracing::instrument::WithSubscriber;

#[tokio::test]
async fn trace_logs_redact_successful_and_rejected_protocol_input() {
    let logs = LogCapture::unbounded();
    let captured = logs.clone();
    let subscriber = tracing_subscriber::fmt()
        .json()
        .with_max_level(tracing::Level::TRACE)
        .with_writer(move || captured.clone())
        .finish();
    async {
        let directory = tempfile::Builder::new().prefix("private-folder-sentinel").tempdir().unwrap();
        support::write_wav(&directory.path().join("private-filename-sentinel.wav"), Some("private-title-sentinel"), 440);
        let app = Folder::scan(directory.path()).unwrap().router();
        let _ = support::search(&app, "private-search-sentinel", None, 20).await;
        let page = support::search(&app, "private-title-sentinel", None, 20).await;
        let response = app.clone().oneshot(Request::get(format!("/v1/audio?source_id=source-{}", page.items[0].id)).header("range", "bytes=0-15").body(Body::empty()).unwrap()).await.unwrap();
        let _ = response.into_body().collect().await.unwrap();
        let _ = support::post(&app, "/v1/search", serde_json::json!({"query":"private-search-sentinel","limit":"private-json-sentinel"})).await;
        let _ = support::post(&app, "/v1/search", serde_json::json!({"query":"demo","limit":20,"private-unknown-field-sentinel":"private-body-sentinel"})).await;
        let _ = app.clone().oneshot(Request::get("/v1/audio?private-query-sentinel=private-query-value-sentinel").body(Body::empty()).unwrap()).await.unwrap();
        let _ = app.clone().oneshot(Request::get("/v1/audio?source_id=missing&source_id=private-duplicate-sentinel").header("authorization", "Bearer private-token-sentinel").body(Body::empty()).unwrap()).await.unwrap();
        let _ = app.oneshot(Request::post("/v1/search").header("content-type", "application/json").body(Body::from(r#"{"query":"private-malformed-sentinel","limit":]"#)).unwrap()).await.unwrap();
    }.with_subscriber(subscriber).await;
    let output = logs.text();
    assert!(output.contains("folder_scan_complete"));
    assert!(output.contains("folder_request_complete"));
    assert!(output.contains("request_id"));
    assert!(output.contains("folder_audio_finished"));
    assert!(
        !output.contains("private-"),
        "trace logs exposed private request or file content"
    );
}

#[tokio::test]
async fn real_network_audio_keeps_scoped_logging_and_server_shutdown_releases_ownership() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let directory = tempfile::Builder::new()
        .prefix("private-network-folder-sentinel")
        .tempdir()
        .unwrap();
    support::write_wav(
        &directory.path().join("private-network-file-sentinel.wav"),
        Some("private-network-title-sentinel"),
        440,
    );
    let logs = LogCapture::unbounded();
    let captured = logs.clone();
    let subscriber = tracing_subscriber::fmt()
        .json()
        .with_max_level(tracing::Level::TRACE)
        .with_writer(move || captured.clone())
        .finish();
    let dispatch = tracing::Dispatch::new(subscriber);
    let app = tracing::dispatcher::with_default(&dispatch, || {
        Folder::scan(directory.path()).unwrap().router()
    });
    let page = support::search(&app, "private-network-title-sentinel", None, 1)
        .with_subscriber(dispatch.clone())
        .await;
    let source_id = format!("source-{}", page.items[0].id);
    let app = app.layer(axum::middleware::from_fn(
        move |request: axum::extract::Request, next: axum::middleware::Next| {
            let dispatch = dispatch.clone();
            async move { next.run(request).with_subscriber(dispatch).await }
        },
    ));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let (shutdown, closed) = tokio::sync::oneshot::channel::<()>();
    let server = tokio::spawn(async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(async {
                let _ = closed.await;
            })
            .await
            .unwrap();
    });
    for (query, status) in [
        (format!("source_id={source_id}"), "206"),
        (
            "private-query-sentinel=private-network-query-sentinel".into(),
            "400",
        ),
    ] {
        let mut connection = tokio::net::TcpStream::connect(address).await.unwrap();
        let request = format!(
            "GET /v1/audio?{query} HTTP/1.1\r\nHost: localhost\r\nRange: bytes=0-15\r\nAuthorization: Bearer private-network-token-sentinel\r\nConnection: close\r\n\r\n"
        );
        connection.write_all(request.as_bytes()).await.unwrap();
        let mut response = Vec::new();
        tokio::time::timeout(
            std::time::Duration::from_secs(2),
            connection.read_to_end(&mut response),
        )
        .await
        .unwrap()
        .unwrap();
        assert!(String::from_utf8_lossy(&response).starts_with(&format!("HTTP/1.1 {status}")));
    }
    shutdown.send(()).unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(2), server)
        .await
        .unwrap()
        .unwrap();
    assert!(tokio::net::TcpStream::connect(address).await.is_err());
    let output = logs.text();
    assert!(output.contains("folder_scan_complete"));
    let entry: serde_json::Value = output
        .lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .find(|entry| entry["fields"]["event"] == "folder_audio_finished")
        .expect("deferred network body must retain its scoped logging subscriber");
    assert_eq!(entry["fields"]["outcome"], "complete");
    assert!(
        entry["spans"]
            .as_array()
            .unwrap()
            .iter()
            .any(|span| span["name"] == "folder_request" && span["request_id"].is_number())
    );
    assert!(!output.contains("private-"));
}
