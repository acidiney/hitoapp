mod support;

use hito_plugin_lrclib::{Config, router};
use serde_json::json;
use support::{Server, TOKEN, Upstream, authorized, record};
use url::Url;

#[tokio::test]
async fn raw_album_metadata_is_bounded_before_blank_normalization() {
    let mut value = record(41);
    value["albumName"] = json!(" ".repeat(2049));
    let (upstream, upstream_server) = Upstream::start(value).await;
    let service = Server::start(
        router(
            Config::new(TOKEN)
                .unwrap()
                .with_endpoint(Url::parse(&upstream_server.url).unwrap())
                .unwrap(),
        )
        .unwrap(),
    )
    .await;
    let response =
        authorized(reqwest::Client::new().post(format!("{}v1/recordings/metadata", service.url)))
            .json(&json!({"recording_id":"41"}))
            .send()
            .await
            .unwrap();
    assert_eq!(response.status(), 502);
    assert_eq!(upstream.seen.lock().unwrap().len(), 1);
    service.close().await;
    upstream_server.close().await;
}

#[tokio::test]
async fn undecodable_timing_only_content_is_unavailable_instead_of_a_false_negative() {
    let mut value = record(41);
    value["plainLyrics"] = serde_json::Value::Null;
    value["syncedLyrics"] = json!("[unsupported:offset]Owned timing-only text");
    let (_, upstream_server) = Upstream::start(value).await;
    let service = Server::start(
        router(
            Config::new(TOKEN)
                .unwrap()
                .with_endpoint(Url::parse(&upstream_server.url).unwrap())
                .unwrap(),
        )
        .unwrap(),
    )
    .await;
    let response = authorized(reqwest::Client::new().post(format!("{}v2/lyrics", service.url)))
        .json(&json!({"recording":{"id":"41","revision":null}}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 502);
    service.close().await;
    upstream_server.close().await;
}

async fn call(
    value: serde_json::Value,
    reply: Option<support::Reply>,
    route: &str,
    payload: serde_json::Value,
) -> (u16, serde_json::Value) {
    let (upstream, server) = Upstream::start(value).await;
    *upstream.response.lock().unwrap() = reply;
    let app = router(
        Config::new(TOKEN)
            .unwrap()
            .with_endpoint(Url::parse(&server.url).unwrap())
            .unwrap(),
    )
    .unwrap();
    let request = axum::http::Request::post(route)
        .header("authorization", format!("Bearer {TOKEN}"))
        .header("x-hito-plugin-protocol", "1")
        .header("content-type", "application/json")
        .body(axum::body::Body::from(payload.to_string()))
        .unwrap();
    use http_body_util::BodyExt;
    use tower::ServiceExt;
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.headers()["cache-control"], "private, no-store");
    let status = response.status().as_u16();
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let value = serde_json::from_slice(&body).unwrap();
    server.close().await;
    (status, value)
}

#[tokio::test]
async fn search_accepts_twenty_valid_summaries_with_large_unused_upstream_lyrics() {
    let records = (1..=20)
        .map(|id| {
            let mut value = record(id);
            value["lyricsfile"] = json!("x".repeat(69_000));
            value
        })
        .collect::<Vec<_>>();
    let upstream_bytes = serde_json::to_vec(&records).unwrap().len();
    assert!(upstream_bytes > 1_048_576 && upstream_bytes < 10 * 1024 * 1024);
    let (status, page) = call(
        json!(records),
        None,
        "/v1/search",
        json!({"query":"Owned river","cursor":null,"limit":20}),
    )
    .await;
    assert_eq!(
        status, 200,
        "valid search metadata is independent of unused upstream lyric bytes"
    );
    let items = page["items"].as_array().unwrap();
    assert_eq!(items.len(), 20);
    for (index, item) in items.iter().enumerate() {
        assert_eq!(item["id"], (index + 1).to_string());
        assert_eq!(item["title"], "Owned river");
        assert_eq!(item["artist"], "Fixture ensemble");
        assert_eq!(item["duration_ms"], 123450);
        assert!(
            item.get("lyricsfile").is_none()
                && item.get("plainLyrics").is_none()
                && item.get("syncedLyrics").is_none()
        );
    }
    assert!(page["next_cursor"].is_null());
    hito_plugin_sdk::SearchPage::validate(&serde_json::from_value(page).unwrap(), 20).unwrap();
}

#[tokio::test]
async fn search_keeps_an_exact_ten_mebibyte_upstream_boundary() {
    for (extra, status) in [(0, 200), (1, 502)] {
        let mut records = (1..=20).map(record).collect::<Vec<_>>();
        let original = serde_json::to_vec(&records).unwrap().len();
        records[0]["lyricsfile"] = json!("x".repeat(10 * 1024 * 1024 + extra - original));
        assert_eq!(
            serde_json::to_vec(&records).unwrap().len(),
            10 * 1024 * 1024 + extra
        );
        let (actual, page) = call(
            json!(records),
            None,
            "/v1/search",
            json!({"query":"Owned river","cursor":null,"limit":20}),
        )
        .await;
        assert_eq!(
            actual, status,
            "the raw upstream byte cap is inclusive and finite"
        );
        if status == 200 {
            assert_eq!(page["items"].as_array().unwrap().len(), 20);
        }
    }
}

#[tokio::test]
async fn hostile_upstream_metadata_pages_bodies_and_statuses_are_contained() {
    use axum::http::StatusCode;
    use support::Reply;
    let metadata = "/v1/recordings/metadata";
    let request = json!({"recording_id":"41"});
    for reply in [
        Reply::Typed("text/html", b"owned HTML".to_vec()),
        Reply::Typed("application/json", b"{".to_vec()),
        Reply::Status(StatusCode::FOUND),
        Reply::Status(StatusCode::INTERNAL_SERVER_ERROR),
    ] {
        let status = call(record(41), Some(reply), metadata, request.clone())
            .await
            .0;
        assert!(matches!(status, 502 | 503));
    }
    let mut oversized = record(41);
    oversized["lyricsfile"] = json!("owned-unused-yaml".repeat(20_000));
    assert_eq!(
        call(oversized, None, metadata, request.clone()).await.0,
        502
    );
    for (field, invalid) in [
        ("id", json!(42)),
        ("trackName", json!(" ".repeat(2049))),
        ("artistName", json!("Owned\tartist")),
        ("duration", json!(-1)),
    ] {
        let mut value = record(41);
        value[field] = invalid;
        assert_eq!(call(value, None, metadata, request.clone()).await.0, 502);
    }
    let search = json!({"query":"Owned", "cursor":null,"limit":1});
    assert_eq!(
        call(
            json!([record(41), record(41)]),
            None,
            "/v1/search",
            search.clone()
        )
        .await
        .0,
        502
    );
    assert_eq!(
        call(
            json!((1..=21).map(record).collect::<Vec<_>>()),
            None,
            "/v1/search",
            search.clone()
        )
        .await
        .0,
        502
    );
    let (status, page) = call(json!([record(41), record(42)]), None, "/v1/search", search).await;
    assert_eq!(status, 200);
    assert_eq!(page["items"].as_array().unwrap().len(), 1);
    assert!(page["next_cursor"].is_null());
    let mut value = record(41);
    value["syncedLyrics"] = serde_json::Value::Null;
    value["plainLyrics"] = json!("Owned\u{0000}private");
    assert_eq!(
        call(
            value,
            None,
            "/v2/lyrics",
            json!({"recording":{"id":"41","revision":null}})
        )
        .await
        .0,
        502
    );
}

#[tokio::test]
async fn invalid_or_unauthenticated_content_requests_do_not_contact_the_upstream() {
    use tower::ServiceExt;
    let (upstream, server) = Upstream::start(record(41)).await;
    let app = router(
        Config::new(TOKEN)
            .unwrap()
            .with_endpoint(Url::parse(&server.url).unwrap())
            .unwrap(),
    )
    .unwrap();
    for route in [
        "/v1/search",
        "/v1/recordings/detail",
        "/v1/recordings/metadata",
        "/v2/lyrics",
        "/v2/lyrics/capabilities",
    ] {
        for (token, protocol) in [
            (None, true),
            (Some("owned-wrong-token"), true),
            (Some(TOKEN), false),
        ] {
            let mut request = if route.ends_with("capabilities") {
                axum::http::Request::get(route)
            } else {
                axum::http::Request::post(route)
            };
            if let Some(token) = token {
                request = request.header("authorization", format!("Bearer {token}"));
            }
            if protocol {
                request = request.header("x-hito-plugin-protocol", "1");
            }
            let response = app
                .clone()
                .oneshot(request.body(axum::body::Body::empty()).unwrap())
                .await
                .unwrap();
            assert!(matches!(response.status().as_u16(), 400 | 401));
        }
    }
    for (route, payload) in [
        (
            "/v1/search",
            json!({"query":"Owned","cursor":"invented-page","limit":1}),
        ),
        (
            "/v1/search",
            json!({"query":"Owned\nprivate","cursor":null,"limit":1}),
        ),
        (
            "/v1/search",
            json!({"query":"Owned","cursor":null,"limit":51}),
        ),
        (
            "/v1/recordings/detail",
            json!({"recording_id":"../private"}),
        ),
        (
            "/v2/lyrics",
            json!({"recording":{"id":"41","revision":null},"private":"forbidden-extra-field"}),
        ),
    ] {
        let request = axum::http::Request::post(route)
            .header("authorization", format!("Bearer {TOKEN}"))
            .header("x-hito-plugin-protocol", "1")
            .header("content-type", "application/json")
            .body(axum::body::Body::from(payload.to_string()))
            .unwrap();
        assert_eq!(app.clone().oneshot(request).await.unwrap().status(), 400);
    }
    assert!(upstream.seen.lock().unwrap().is_empty());
    server.close().await;
}

#[tokio::test]
async fn genuine_instrumental_absence_and_unknown_revisions_echo_the_exact_reference() {
    let mut value = record(41);
    value["instrumental"] = json!(true);
    value["plainLyrics"] = serde_json::Value::Null;
    value["syncedLyrics"] = serde_json::Value::Null;
    let request = json!({"recording":{"id":"41","revision":null}});
    let (status, response) = call(value, None, "/v2/lyrics", request.clone()).await;
    assert_eq!(status, 200);
    assert_eq!(
        response,
        json!({"recording":request["recording"],"lyrics":null})
    );
    let request = json!({"recording":{"id":"41","revision":"unknown-owned-revision"}});
    let (status, response) = call(
        record(41),
        Some(support::Reply::Status(
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
        )),
        "/v2/lyrics",
        request.clone(),
    )
    .await;
    assert_eq!(status, 200);
    assert_eq!(
        response,
        json!({"recording":request["recording"],"lyrics":null})
    );
    let (status, response) = call(
        record(41),
        Some(support::Reply::Status(axum::http::StatusCode::NOT_FOUND)),
        "/v2/lyrics",
        json!({"recording":{"id":"42","revision":null}}),
    )
    .await;
    assert_eq!(status, 200);
    assert_eq!(
        response,
        json!({"recording":{"id":"42","revision":null},"lyrics":null})
    );
}
