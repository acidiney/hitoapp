use super::fixture::{Fixture, ROOT, raw};
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use serde_json::{Value, json};

#[tokio::test]
async fn malformed_snapshot_paths_return_safe_json_and_unknown_pages_are_not_found() {
    let f = Fixture::start().await;
    let snapshot = f.begin().await;
    let id = snapshot["id"].as_str().unwrap();
    for suffix in [
        "private-path-sentinel/pages/0".into(),
        format!("{id}/pages/private-ordinal-sentinel"),
        format!("{id}/pages/-1"),
        format!("{id}/pages/18446744073709551616"),
        format!("{id}/pages/%FF"),
    ] {
        let reply = f
            .call("GET", &format!("{ROOT}/{suffix}"), Value::Null)
            .await;
        assert_eq!(reply.status, StatusCode::UNPROCESSABLE_ENTITY);
        reply.private();
        assert_eq!(
            reply.json(),
            json!({"error":"Catalog snapshot path does not match the expected fields"})
        );
    }
    for suffix in [
        format!("{}/pages/0", uuid::Uuid::new_v4()),
        format!("{id}/pages/1"),
        format!("{id}/pages/18446744073709551615"),
    ] {
        let reply = f
            .call("GET", &format!("{ROOT}/{suffix}"), Value::Null)
            .await;
        assert_eq!(reply.status, StatusCode::NOT_FOUND);
        assert_eq!(reply.json(), json!({"error":"Resource not found"}));
    }
    for id in ["private-release-sentinel", "%FF"] {
        let reply = f.call("DELETE", &format!("{ROOT}/{id}"), Value::Null).await;
        assert_eq!(reply.status, StatusCode::UNPROCESSABLE_ENTITY);
        reply.private();
        assert_eq!(
            reply.json(),
            json!({"error":"Catalog snapshot path does not match the expected fields"})
        );
    }
    f.services.catalog_replication.close().await.unwrap();
}

#[tokio::test]
async fn snapshot_json_uses_the_shared_strict_bounded_request_contract() {
    let f = Fixture::start().await;
    let cases = [
        ("application/json", "{".to_owned(), StatusCode::BAD_REQUEST),
        (
            "application/json",
            "null".into(),
            StatusCode::UNPROCESSABLE_ENTITY,
        ),
        (
            "application/json",
            "{}".into(),
            StatusCode::UNPROCESSABLE_ENTITY,
        ),
        (
            "application/json",
            json!({"library_id":f.library,"private-body-sentinel":true}).to_string(),
            StatusCode::UNPROCESSABLE_ENTITY,
        ),
        (
            "application/json",
            json!({"library_id":42}).to_string(),
            StatusCode::UNPROCESSABLE_ENTITY,
        ),
        (
            "text/plain",
            json!({"library_id":f.library}).to_string(),
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
        ),
        (
            "application/json",
            json!({"library_id":"private-body-sentinel".repeat(20_000)}).to_string(),
            StatusCode::PAYLOAD_TOO_LARGE,
        ),
    ];
    for (content_type, body, expected) in cases {
        let reply = raw(
            &f.app,
            Request::builder()
                .method("POST")
                .uri(ROOT)
                .header("authorization", format!("Bearer {}", f.token))
                .header("content-type", content_type)
                .body(Body::from(body))
                .unwrap(),
        )
        .await;
        assert_eq!(reply.status, expected);
        reply.private();
        assert!(reply.json()["error"].is_string());
        assert!(!reply.json().to_string().contains("private-body-sentinel"));
    }
    let absent = f
        .call("POST", ROOT, json!({"library_id":uuid::Uuid::new_v4()}))
        .await;
    assert_eq!(absent.status, StatusCode::NOT_FOUND);
    assert_eq!(f.begin().await["recording_count"], 0);
    f.services.catalog_replication.close().await.unwrap();
}
