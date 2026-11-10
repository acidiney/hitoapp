use super::fixture::{Fixture, ROOT, raw};
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use serde_json::json;

#[tokio::test]
async fn native_alias_callers_omit_origin_and_browser_mutations_keep_origin_policy() {
    let f = Fixture::start().await;
    for origin in [
        "https://hostile.example",
        "http://native-backend-alias:8080",
    ] {
        let reply = raw(
            &f.app,
            Request::builder()
                .method("POST")
                .uri(ROOT)
                .header("origin", origin)
                .header("authorization", format!("Bearer {}", f.token))
                .header("x-hito-request", "1")
                .header("content-type", "application/json")
                .body(Body::from(json!({"library_id":f.library}).to_string()))
                .unwrap(),
        )
        .await;
        assert_eq!(reply.status, StatusCode::FORBIDDEN);
        assert_eq!(
            reply.json(),
            json!({"error":"Request origin could not be verified"})
        );
    }
    let native = raw(
        &f.app,
        Request::builder()
            .method("POST")
            .uri(ROOT)
            .header("host", "native-backend-alias:8080")
            .header("authorization", format!("Bearer {}", f.token))
            .header("x-hito-request", "1")
            .header("content-type", "application/json")
            .body(Body::from(json!({"library_id":f.library}).to_string()))
            .unwrap(),
    )
    .await;
    assert_eq!(native.status, StatusCode::OK);
    native.private();
    let snapshot = native.json();
    let id = snapshot["id"].as_str().unwrap();
    let cookie_page = raw(
        &f.app,
        Request::builder()
            .uri(format!("{ROOT}/{id}/pages/0"))
            .header("cookie", format!("hito_session={}", f.token))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(cookie_page.status, StatusCode::OK);
    cookie_page.private();
    let rejected = raw(
        &f.app,
        Request::builder()
            .method("DELETE")
            .uri(format!("{ROOT}/{id}"))
            .header("origin", "https://hostile.example")
            .header("authorization", format!("Bearer {}", f.token))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(rejected.status, StatusCode::FORBIDDEN);
    let browser = raw(
        &f.app,
        Request::builder()
            .method("POST")
            .uri(ROOT)
            .header("origin", "http://localhost:8080")
            .header("cookie", format!("hito_session={}", f.token))
            .header("x-hito-request", "1")
            .header("content-type", "application/json")
            .body(Body::from(json!({"library_id":f.library}).to_string()))
            .unwrap(),
    )
    .await;
    assert_eq!(browser.status, StatusCode::OK);
    assert_eq!(browser.json(), snapshot);
    let no_csrf = raw(
        &f.app,
        Request::builder()
            .method("DELETE")
            .uri(format!("{ROOT}/{id}"))
            .header("origin", "http://localhost:8080")
            .header("cookie", format!("hito_session={}", f.token))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(no_csrf.status, StatusCode::FORBIDDEN);
    let release = raw(
        &f.app,
        Request::builder()
            .method("DELETE")
            .uri(format!("{ROOT}/{id}"))
            .header("origin", "http://localhost:8080")
            .header("cookie", format!("hito_session={}", f.token))
            .header("x-hito-request", "1")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(release.status, StatusCode::NO_CONTENT);
    f.services.catalog_replication.close().await.unwrap();
}
