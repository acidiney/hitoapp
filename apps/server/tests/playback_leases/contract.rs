use super::fixture::Case;
use axum::http::StatusCode;
use http_body_util::BodyExt;
use serde_json::Value;

#[tokio::test]
async fn served_and_generated_contracts_describe_the_actual_rust_lease_routes() {
    let f = Case::new().await;
    let response = f.request("GET", "/api/v1/openapi.json", &[], None).await;
    let status = response.status();
    let document: Value =
        serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes()).unwrap();
    f.close().await;
    let generated = serde_json::to_value(hito_server::openapi()).unwrap();
    let committed: Value = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../docs/openapi.json"
    )))
    .unwrap();
    assert_eq!(status, StatusCode::OK);
    assert!(
        document == generated && generated == committed,
        "Served OpenAPI and docs/openapi.json must come from the actual Rust routes and types"
    );
    for (path, method) in [
        ("/api/v1/playback/leases", "post"),
        ("/api/v1/playback/leases/{id}", "get"),
        ("/api/v1/playback/leases/{id}", "delete"),
        ("/api/v1/playback/leases/{id}/audio", "get"),
        ("/api/v1/playback/leases/{id}/audio", "head"),
    ] {
        let operation = &document["paths"][path][method];
        assert!(operation.is_object() && operation["security"].is_array());
    }
    let schemas = &document["components"]["schemas"];
    assert_eq!(schemas["BeginPlaybackLease"]["additionalProperties"], false);
    assert_eq!(schemas["PlaybackLease"]["additionalProperties"], false);
    assert_eq!(
        schemas["PlaybackLease"]["properties"]
            .as_object()
            .unwrap()
            .len(),
        2
    );
}
