use super::fixture::{Case, credentials};
use axum::http::{StatusCode, header};
use hito_core::PlaybackLease;
use http_body_util::BodyExt;

async fn audio_path(f: &Case) -> String {
    let response = f.begin().await;
    assert_eq!(response.status(), StatusCode::OK);
    let lease: PlaybackLease =
        serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes()).unwrap();
    format!("/api/v1/playback/leases/{}/audio", lease.id)
}

#[tokio::test]
async fn original_ranges_head_and_if_range_share_the_existing_representation_policy() {
    let f = Case::new().await;
    let path = audio_path(&f).await;
    let total = f.original.len();
    let mut evidence = Vec::new();
    for (range, expected) in [
        ("bytes=0-0", 0..1),
        ("bytes=7-19", 7..20),
        ("bytes=-9", total - 9..total),
        ("bytes=5-", 5..total),
        ("bytes=9-18446744073709551615", 9..total),
    ] {
        let response = f.request("GET", &path, &[("range", range)], None).await;
        let headers = response.headers();
        let valid = response.status() == StatusCode::PARTIAL_CONTENT
            && headers[header::CONTENT_LENGTH] == expected.len().to_string()
            && headers[header::CONTENT_RANGE]
                == format!("bytes {}-{}/{total}", expected.start, expected.end - 1)
            && headers[header::ACCEPT_RANGES] == "bytes"
            && headers[header::CACHE_CONTROL] == "private, no-store"
            && !headers.contains_key(header::ETAG);
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        evidence.push(valid && bytes.as_ref() == &f.original[expected]);
    }
    for headers in [
        vec![],
        vec![("range", "items=1-2")],
        vec![("range", "bytes=4-2"), ("if-range", "\"unverified\"")],
    ] {
        let response = f.request("GET", &path, &headers, None).await;
        let valid = response.status() == StatusCode::OK
            && !response.headers().contains_key(header::CONTENT_RANGE);
        evidence.push(
            valid
                && response
                    .into_body()
                    .collect()
                    .await
                    .unwrap()
                    .to_bytes()
                    .as_ref()
                    == f.original.as_slice(),
        );
    }
    let head = f
        .request("HEAD", &path, &[("range", "bytes=-9")], None)
        .await;
    let head_valid = head.status() == StatusCode::OK
        && head.headers()[header::CONTENT_LENGTH] == total.to_string();
    evidence.push(
        head_valid
            && head
                .into_body()
                .collect()
                .await
                .unwrap()
                .to_bytes()
                .is_empty(),
    );
    f.close().await;
    assert!(evidence.iter().all(|valid| *valid), "{evidence:?}");
}

#[tokio::test]
async fn invalid_and_unsatisfiable_ranges_authorize_before_returning_416() {
    let f = Case::new().await;
    let path = audio_path(&f).await;
    let total = f.original.len();
    let other = f.services.identity.login(credentials()).await.unwrap();
    let mut evidence = Vec::new();
    for range in [
        "bytes=4-2".into(),
        "bytes=-0".into(),
        "bytes=0-1,4-5".into(),
        format!("bytes={total}-"),
        "x".repeat(1025),
    ] {
        let response = f.request("GET", &path, &[("range", &range)], None).await;
        let valid = response.status() == StatusCode::RANGE_NOT_SATISFIABLE
            && response.headers()[header::CONTENT_RANGE] == format!("bytes */{total}")
            && response.headers()[header::CONTENT_LENGTH] == "0"
            && response.headers()[header::CACHE_CONTROL] == "private, no-store";
        evidence.push(
            valid
                && response
                    .into_body()
                    .collect()
                    .await
                    .unwrap()
                    .to_bytes()
                    .is_empty(),
        );
        evidence.push(
            f.request_as(None, "GET", &path, &[("range", &range)], None)
                .await
                .status()
                == StatusCode::UNAUTHORIZED,
        );
        evidence.push(
            f.request_as(Some(&other.token), "GET", &path, &[("range", &range)], None)
                .await
                .status()
                == StatusCode::NOT_FOUND,
        );
    }
    let duplicate = f
        .request(
            "GET",
            &path,
            &[("range", "bytes=0-1"), ("range", "bytes=4-5")],
            None,
        )
        .await
        .status();
    f.close().await;
    assert!(evidence.iter().all(|valid| *valid), "{evidence:?}");
    assert_eq!(duplicate, StatusCode::RANGE_NOT_SATISFIABLE);
}

#[tokio::test]
async fn shared_transport_extraction_preserves_ordinary_source_audio() {
    let f = Case::new().await;
    let path = format!("/api/v1/sources/{}/audio", f.source);
    let response = f
        .request("GET", &path, &[("range", "bytes=7-19")], None)
        .await;
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let unchanged = bytes.as_ref() == &f.original[7..20];
    let head = f
        .request("HEAD", &path, &[("range", "bytes=7-19")], None)
        .await;
    let head_valid = head.status() == StatusCode::OK
        && head.headers()[header::CONTENT_LENGTH] == f.original.len().to_string();
    f.close().await;
    assert_eq!(status, StatusCode::PARTIAL_CONTENT);
    assert!(unchanged && head_valid);
}
