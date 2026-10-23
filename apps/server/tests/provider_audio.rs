use audio_fixture::AudioLibrary;
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use hito_core::test_support::provider_audio as audio_fixture;
use http_body_util::BodyExt;
use tower::ServiceExt;

#[test]
fn audio_contract_describes_binary_get_and_bodyless_head_with_distinct_operations() {
    let contract = serde_json::to_value(hito_server::openapi()).unwrap();
    let path = &contract["paths"]["/api/v1/sources/{id}/audio"];
    assert_eq!(
        contract["components"]["schemas"]["AudioStream"]["type"],
        "string"
    );
    assert_eq!(
        contract["components"]["schemas"]["AudioStream"]["format"],
        "binary"
    );
    assert!(path["head"]["responses"]["200"].is_object());
    assert!(path["head"]["responses"]["200"].get("content").is_none());
    assert_ne!(path["get"]["operationId"], path["head"]["operationId"]);
}

#[tokio::test]
async fn browser_audio_endpoint_relays_provider_bytes_with_private_headers_and_head() {
    let f = AudioLibrary::start().await;
    let mut headers = audio_fixture::headers();
    for (name, value) in [
        ("set-cookie", "provider-private-cookie=private"),
        (
            "location",
            "https://private.invalid/?token=provider-private-signed",
        ),
        (
            "content-disposition",
            "attachment; filename=provider-private-file.wav",
        ),
        ("x-provider-private", "provider-private-value"),
    ] {
        headers.insert(name, value.parse().unwrap());
    }
    f.server
        .audio(StatusCode::OK, headers, f.original.clone())
        .await;
    let app = hito_server::router(f.hito.clone());
    let session = f
        .hito
        .identity
        .login(hito_core::Credentials {
            username: "admin".into(),
            password: "indexing test password".into(),
        })
        .await
        .unwrap();
    for method in ["GET", "HEAD"] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(method)
                    .uri(format!("/api/v1/sources/{}/audio", f.source.id))
                    .header("authorization", format!("Bearer {}", session.token))
                    .header("cookie", "private-browser-cookie=secret")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()["cache-control"], "private, no-store");
        assert_eq!(response.headers()["content-type"], "audio/wav");
        assert_eq!(
            response.headers()["content-length"],
            f.original.len().to_string()
        );
        for name in [
            "set-cookie",
            "location",
            "content-disposition",
            "x-provider-private",
        ] {
            assert!(response.headers().get(name).is_none());
        }
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        assert_eq!(
            bytes.as_ref(),
            if method == "HEAD" {
                &[]
            } else {
                f.original.as_slice()
            }
        );
    }
    assert_eq!(f.server.requests("v1/audio").await[1].1["_method"], "HEAD");
    for (headers, _) in f.server.requests("v1/audio").await {
        assert_eq!(headers["authorization"], "Bearer private-audio-token");
        assert!(headers.get("cookie").is_none());
    }
}

#[tokio::test]
async fn browser_seeks_relay_validated_ranges_and_ignore_unmatched_if_range_and_head_ranges() {
    let f = AudioLibrary::start().await;
    let app = hito_server::router(f.hito.clone());
    let session = f
        .hito
        .identity
        .login(hito_core::Credentials {
            username: "admin".into(),
            password: "indexing test password".into(),
        })
        .await
        .unwrap();
    let total = f.original.len();
    let mut headers = audio_fixture::headers();
    headers.insert(
        "content-range",
        format!("bytes 44-63/{total}").parse().unwrap(),
    );
    f.server
        .audio(
            StatusCode::PARTIAL_CONTENT,
            headers,
            f.original[44..64].to_vec(),
        )
        .await;
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/api/v1/sources/{}/audio", f.source.id))
                .header("authorization", format!("Bearer {}", session.token))
                .header("range", "bytes=44-63")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::PARTIAL_CONTENT);
    assert_eq!(
        response.headers()["content-range"],
        format!("bytes 44-63/{total}")
    );
    assert_eq!(response.headers()["content-length"], "20");
    assert_eq!(
        response.into_body().collect().await.unwrap().to_bytes(),
        &f.original[44..64]
    );
    assert_eq!(
        f.server.requests("v1/audio").await[0].0["range"],
        "bytes=44-63"
    );

    let mut headers = audio_fixture::headers();
    headers.insert("content-range", format!("BYTES */{total}").parse().unwrap());
    f.server
        .audio(StatusCode::RANGE_NOT_SATISFIABLE, headers, Vec::new())
        .await;
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/api/v1/sources/{}/audio", f.source.id))
                .header("authorization", format!("Bearer {}", session.token))
                .header("range", "bytes=999999-")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::RANGE_NOT_SATISFIABLE);
    assert_eq!(
        response.headers()["content-range"],
        format!("bytes */{total}")
    );
    assert_eq!(
        response
            .into_body()
            .collect()
            .await
            .unwrap()
            .to_bytes()
            .len(),
        0
    );

    f.ready().await;
    for (method, if_range) in [("GET", true), ("HEAD", false)] {
        let mut request = Request::builder()
            .method(method)
            .uri(format!("/api/v1/sources/{}/audio", f.source.id))
            .header("authorization", format!("Bearer {}", session.token))
            .header("range", "bytes=44-63");
        if if_range {
            request = request.header("if-range", "\"not-a-current-validator\"");
        }
        let response = app
            .clone()
            .oneshot(request.body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()["content-length"], total.to_string());
        assert!(!response.headers().contains_key("content-range"));
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        assert_eq!(
            bytes.as_ref(),
            if method == "HEAD" {
                &[]
            } else {
                f.original.as_slice()
            }
        );
    }
    for (headers, _) in &f.server.requests("v1/audio").await[2..] {
        assert!(!headers.contains_key("range"));
        assert!(!headers.contains_key("if-range"));
    }
}
