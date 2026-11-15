use axum::http::{HeaderMap, StatusCode};
use hito_core::test_support::{content_service::ProviderServer, hang_guard::hang_guard};
use std::time::Duration;

#[tokio::test]
async fn explicit_range_end_is_bounded_within_maximum_chunk() {
    let original: Vec<u8> = (0..=255).collect();
    let (server, client) = hang_guard("owned fake audio server setup", async {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let client = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .expect("the owned fake range client must initialize");
        let server = ProviderServer::start().await;
        server.range_chunks(81).await;
        server
            .audio(StatusCode::OK, HeaderMap::new(), original.clone())
            .await;
        (server, client)
    })
    .await;

    let response = tokio::time::timeout(Duration::from_secs(5), async {
        let response = client
            .get(format!("{}v1/audio", server.url))
            .header(reqwest::header::RANGE, "bytes=0-1")
            .send()
            .await?;
        let status = response.status();
        let content_range = response
            .headers()
            .get(reqwest::header::CONTENT_RANGE)
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned);
        let content_length = response
            .headers()
            .get(reqwest::header::CONTENT_LENGTH)
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned);
        let body = response.bytes().await?.to_vec();
        Ok::<_, reqwest::Error>((status, content_range, content_length, body))
    })
    .await;
    server.shutdown().await;

    let result = match response {
        Ok(Ok(response)) => Some(response),
        Ok(Err(_)) | Err(_) => None,
    };
    assert!(result.is_some(), "the owned fake range request must complete");
    let (status, content_range, content_length, body) = result.unwrap();
    assert_eq!(status, StatusCode::PARTIAL_CONTENT);
    assert_eq!(content_range.as_deref(), Some("bytes 0-1/256"));
    assert_eq!(content_length.as_deref(), Some("2"));
    assert_eq!(body.as_slice(), &original[..2]);
}
