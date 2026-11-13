use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use futures_util::FutureExt;
use hito_core::test_support::artwork_admission::ArtworkAdmission;
use http_body_util::BodyExt;
use serde_json::Value;
use std::{panic::AssertUnwindSafe, time::Duration};
use tower::ServiceExt;

#[tokio::test]
async fn saturated_artwork_admission_has_a_typed_retryable_response() {
    let fixture = Box::pin(ArtworkAdmission::saturated()).await;
    let workflow = AssertUnwindSafe(Box::pin(async {
        let app = hito_server::router(fixture.hito.clone());
        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!(
                        "/api/v1/recordings/{}/artwork/fetch",
                        fixture.recording
                    ))
                    .header("authorization", format!("Bearer {}", fixture.token))
                    .header("x-hito-request", "1")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
        let retry_after: u64 = response.headers()["retry-after"]
            .to_str()
            .unwrap()
            .parse()
            .unwrap();
        assert!((1..=30).contains(&retry_after));
        let body = response.into_body().collect().await.unwrap().to_bytes();
        let body: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(body["code"], "artwork_admission_busy");
    }))
    .catch_unwind();
    let outcome = tokio::time::timeout(Duration::from_secs(30), workflow).await;
    Box::pin(fixture.close()).await;
    let outcome = outcome.expect("artwork admission HTTP workflow exceeded its 30-second budget");
    if let Err(payload) = outcome {
        std::panic::resume_unwind(payload);
    }
}

#[tokio::test]
async fn release_reads_preserve_recordings_while_artwork_admission_is_saturated() {
    let fixture = Box::pin(ArtworkAdmission::saturated()).await;
    let workflow = AssertUnwindSafe(Box::pin(async {
        let before = fixture
            .hito
            .catalog
            .recordings(&fixture.actor, hito_core::Search::default())
            .await
            .unwrap();
        assert_eq!(before.total, 1);
        let recording = before
            .items
            .iter()
            .find(|item| item.id == fixture.recording)
            .unwrap();
        let release = recording.release_id.as_ref().unwrap();
        let recording_before = serde_json::to_value(recording).unwrap();
        let app = hito_server::router(fixture.hito.clone());
        let detail = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(format!("/api/v1/releases/{release}"))
                    .header("authorization", format!("Bearer {}", fixture.token))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(detail.status(), StatusCode::OK);
        let detail = detail.into_body().collect().await.unwrap().to_bytes();
        let detail: Value = serde_json::from_slice(&detail).unwrap();
        assert_eq!(detail["release"]["id"], release.as_str());
        assert_eq!(detail["release"]["title"], recording_before["album"]);
        assert!(
            detail["tracks"]["items"]
                .as_array()
                .unwrap()
                .iter()
                .any(|track| track["id"] == fixture.recording)
        );
        let image = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(format!("/api/v1/releases/{release}/artwork"))
                    .header("authorization", format!("Bearer {}", fixture.token))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(image.status(), StatusCode::NOT_FOUND);
        assert_eq!(image.headers()["cache-control"], "private, no-store");
        let image = image.into_body().collect().await.unwrap().to_bytes();
        let image: Value = serde_json::from_slice(&image).unwrap();
        assert_eq!(image, serde_json::json!({"error": "Resource not found"}));
        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/releases/{release}/artwork/fetch"))
                    .header("authorization", format!("Bearer {}", fixture.token))
                    .header("x-hito-request", "1")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
        let retry_after: u64 = response.headers()["retry-after"]
            .to_str()
            .unwrap()
            .parse()
            .unwrap();
        assert!((1..=30).contains(&retry_after));
        let body = response.into_body().collect().await.unwrap().to_bytes();
        let body: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(body["code"], "artwork_admission_busy");
        let after = fixture
            .hito
            .catalog
            .recordings(&fixture.actor, hito_core::Search::default())
            .await
            .unwrap();
        assert_eq!(after.total, before.total);
        let recording_after = after
            .items
            .iter()
            .find(|item| item.id == fixture.recording)
            .unwrap();
        assert_eq!(
            serde_json::to_value(recording_after).unwrap(),
            recording_before
        );
    }))
    .catch_unwind();
    let outcome = tokio::time::timeout(Duration::from_secs(30), workflow).await;
    Box::pin(fixture.close()).await;
    let outcome = outcome.expect("release artwork control exceeded its 30-second budget");
    if let Err(payload) = outcome {
        std::panic::resume_unwind(payload);
    }
}
