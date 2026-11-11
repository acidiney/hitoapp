use super::setup::Prepared;
use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use serde_json::Value;
use std::collections::BTreeSet;
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
async fn actual_automatic_response_matches_resolved_rust_schema_and_adds_no_changes_to_old_contracts()
 {
    let prepared = Prepared::new().await;
    let mut request = Request::get("/api/v1/openapi.json")
        .body(Body::empty())
        .unwrap();
    *request.headers_mut() = prepared.library.account.headers();
    let response = prepared.library.app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let document: Value =
        serde_json::from_slice(&to_bytes(response.into_body(), 2_097_152).await.unwrap()).unwrap();
    let operation = &document["paths"]["/api/v1/recordings/{id}/lyrics/automatic"]["post"];
    assert_eq!(operation["operationId"], "automatic_recording_lyrics");
    let body = resolve(
        &document,
        &operation["requestBody"]["content"]["application/json"]["schema"],
    );
    assert_eq!(body["type"], "object");
    assert_eq!(body["additionalProperties"], false);
    let response = prepared.read().await;
    let lookup = resolve(
        &document,
        &operation["responses"]["200"]["content"]["application/json"]["schema"],
    );
    let variant = lookup["oneOf"]
        .as_array()
        .unwrap()
        .iter()
        .map(|schema| resolve(&document, schema))
        .find(|schema| schema["properties"]["status"]["enum"][0] == response["status"])
        .unwrap();
    let actual: BTreeSet<_> = response.as_object().unwrap().keys().collect();
    let described: BTreeSet<_> = variant["properties"].as_object().unwrap().keys().collect();
    assert_eq!(actual, described);
    assert!(variant["properties"].get("selector").is_none());
    let lyric = resolve(&document, &variant["properties"]["document"]);
    for key in response["document"].as_object().unwrap().keys() {
        assert!(lyric["properties"].get(key).is_some());
    }
    assert_eq!(
        response["provider_recording"],
        response["document"]["recording"]
    );
    let baseline: Value = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../docs/openapi.json"
    )))
    .unwrap();
    for (path, old) in baseline["paths"].as_object().unwrap() {
        assert!(
            document["paths"][path] == *old,
            "An automatic-only route must preserve existing path contracts"
        );
    }
    for (name, old) in baseline["components"]["schemas"].as_object().unwrap() {
        assert!(
            document["components"]["schemas"][name] == *old,
            "Automatic result types must preserve existing schema meanings"
        );
    }
    prepared.finish().await;
}
