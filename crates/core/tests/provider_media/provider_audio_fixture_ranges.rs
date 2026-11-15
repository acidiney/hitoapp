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

#[tokio::test]
async fn single_ranges_return_their_resolved_selection() {
    struct Response {
        status: StatusCode,
        content_range: Option<String>,
        content_length: Option<String>,
        accept_ranges: Option<String>,
        body: Vec<u8>,
    }

    let original: Vec<u8> = (0..=255).collect();
    let (server, client) = hang_guard("owned fake range matrix setup", async {
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

    let cases = [
        (
            "finite",
            "GET",
            Some("bytes=10-90"),
            StatusCode::PARTIAL_CONTENT,
            Some("bytes 10-90/256"),
            81,
            10..91,
        ),
        (
            "open_ended",
            "GET",
            Some("bytes=10-"),
            StatusCode::PARTIAL_CONTENT,
            Some("bytes 10-90/256"),
            81,
            10..91,
        ),
        (
            "suffix",
            "GET",
            Some("bytes=-4"),
            StatusCode::PARTIAL_CONTENT,
            Some("bytes 252-255/256"),
            4,
            252..256,
        ),
        (
            "eof_clamp",
            "GET",
            Some("bytes=250-999"),
            StatusCode::PARTIAL_CONTENT,
            Some("bytes 250-255/256"),
            6,
            250..256,
        ),
        (
            "first_at_eof",
            "GET",
            Some("bytes=256-"),
            StatusCode::RANGE_NOT_SATISFIABLE,
            Some("bytes */256"),
            0,
            0..0,
        ),
        (
            "zero_suffix",
            "GET",
            Some("bytes=-0"),
            StatusCode::RANGE_NOT_SATISFIABLE,
            Some("bytes */256"),
            0,
            0..0,
        ),
        (
            "reversed",
            "GET",
            Some("bytes=20-10"),
            StatusCode::RANGE_NOT_SATISFIABLE,
            Some("bytes */256"),
            0,
            0..0,
        ),
        (
            "leading_plus",
            "GET",
            Some("bytes=+0-1"),
            StatusCode::RANGE_NOT_SATISFIABLE,
            Some("bytes */256"),
            0,
            0..0,
        ),
        (
            "multiple_ranges",
            "GET",
            Some("bytes=0-1,4-5"),
            StatusCode::RANGE_NOT_SATISFIABLE,
            Some("bytes */256"),
            0,
            0..0,
        ),
        ("no_range", "GET", None, StatusCode::OK, None, 256, 0..256),
        ("head", "HEAD", None, StatusCode::OK, None, 256, 0..0),
    ];
    let mut responses = Vec::with_capacity(cases.len());
    for (name, method, range, _, _, _, _) in &cases {
        let response = tokio::time::timeout(Duration::from_secs(5), async {
            let method = if *method == "HEAD" {
                reqwest::Method::HEAD
            } else {
                reqwest::Method::GET
            };
            let mut request = client.request(method, format!("{}v1/audio", server.url));
            if let Some(range) = *range {
                request = request.header(reqwest::header::RANGE, range);
            }
            let response = request.send().await?;
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
            let accept_ranges = response
                .headers()
                .get(reqwest::header::ACCEPT_RANGES)
                .and_then(|value| value.to_str().ok())
                .map(str::to_owned);
            let body = response.bytes().await?.to_vec();
            Ok::<_, reqwest::Error>(Response {
                status,
                content_range,
                content_length,
                accept_ranges,
                body,
            })
        })
        .await;
        responses.push((*name, response.ok().and_then(Result::ok)));
    }
    server.shutdown().await;

    for ((name, _, _, status, content_range, content_length, body_range), (_, response)) in
        cases.iter().zip(responses)
    {
        let Some(response) = response else {
            panic!("{name}: the owned range request must complete");
        };
        assert_eq!(response.status, *status, "{name}");
        assert_eq!(response.content_range.as_deref(), *content_range, "{name}");
        let expected_content_length = content_length.to_string();
        assert_eq!(
            response.content_length.as_deref(),
            Some(expected_content_length.as_str()),
            "{name}"
        );
        assert_eq!(response.accept_ranges.as_deref(), Some("bytes"), "{name}");
        assert!(
            response.body.as_slice() == &original[body_range.clone()],
            "{name}: body interval mismatch"
        );
    }
}
