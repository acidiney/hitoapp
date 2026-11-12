use super::setup::Prepared;
use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use serde_json::Value;
use std::time::Duration;
use tower::ServiceExt;

fn resolve<'a>(document: &'a Value, schema: &'a Value) -> &'a Value {
    match schema["$ref"].as_str() {
        Some(reference) => {
            &document["components"]["schemas"]
                [reference.strip_prefix("#/components/schemas/").unwrap()]
        }
        None => schema,
    }
}

#[tokio::test]
async fn public_review_schema_describes_returned_sdk_metadata_and_preserves_the_host_metadata_view()
{
    let prepared = Prepared::new().await;
    let mut request = Request::get("/api/v1/openapi.json")
        .body(Body::empty())
        .unwrap();
    *request.headers_mut() = prepared.library.account.headers();
    let response = tokio::time::timeout(
        Duration::from_secs(5),
        prepared.library.app.clone().oneshot(request),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = to_bytes(response.into_body(), 1048576).await.unwrap();
    let document: Value = serde_json::from_slice(&bytes).unwrap();
    let (status, review) = prepared
        .library
        .account
        .request(
            &prepared.library.app,
            "POST",
            &format!("{}/identities/review", prepared.base()),
            prepared.candidate.clone(),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let candidate = resolve(
        &document,
        &document["components"]["schemas"]["RecordingLyricReview"]["properties"]["candidate"],
    );
    for key in review["candidate"].as_object().unwrap().keys() {
        assert!(
            candidate["properties"].get(key).is_some(),
            "The lyric review schema must describe every returned SDK metadata field"
        );
    }
    assert!(candidate["properties"].get("recording").is_some());
    assert!(candidate["properties"].get("recording_id").is_none());
    let metadata = prepared
        .library
        .account
        .get(
            &prepared.library.app,
            &format!("/api/v1/recordings/{}/metadata", prepared.library.recording),
        )
        .await;
    let host = &document["components"]["schemas"]["RecordingMetadata"];
    let baseline: Value = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../docs/openapi.json"
    )))
    .unwrap();
    assert_eq!(
        host, &baseline["components"]["schemas"]["RecordingMetadata"],
        "The complete existing host metadata component must remain unchanged"
    );
    for key in metadata.as_object().unwrap().keys() {
        assert!(
            host["properties"].get(key).is_some(),
            "The existing host metadata schema must keep describing its own response"
        );
    }
    assert!(host["properties"].get("recording_id").is_some());
    assert!(host["properties"].get("recording").is_none());
    prepared.library.assert_original();
    prepared.provider.finish().await;
}
