use crate::{artist_identity_fixture::Library, artist_review_fixture::exchange};
use axum::{
    body::{Body, to_bytes},
    http::{HeaderMap, Request, StatusCode},
};
use futures_util::FutureExt;
use serde_json::{Value, json};
use std::{
    panic::{AssertUnwindSafe, resume_unwind},
    time::Duration,
};
use tower::ServiceExt;
use uuid::Uuid;

#[tokio::test]
async fn http_identity_transport_validates_query_json_size_and_browser_mutation_origin() {
    let library = Library::prepare().await;
    let result = AssertUnwindSafe(exercise(&library)).catch_unwind().await;
    library.finish().await;
    if let Err(panic) = result {
        resume_unwind(panic);
    }
}

async fn exercise(library: &Library) {
    let (choose, receipt) = library.select(&library.local).await;
    library.select(&library.imported).await;
    let path = format!("/api/v1/recordings/{}/artist-identity", library.local);
    let reset_path = format!("{path}/reset");
    let reset = json!({"request_id": Uuid::new_v4(), "expected_revision": 1});
    let detail_path = format!(
        "/api/v1/artist-identity-groups/{}",
        receipt["selection"]["group_id"].as_str().unwrap()
    );
    let before = library.snapshot().await;
    let choice_before = library.owner.get(&library.app, &path).await;
    let detail_before = library.owner.get(&library.app, &detail_path).await;
    let provider_before = library.provider_counts().await;
    for (route, expected) in [
        (
            "/api/v1/artist-identity-groups?offset=invalid".into(),
            StatusCode::BAD_REQUEST,
        ),
        (
            "/api/v1/artist-identity-groups?limit=9223372036854775808".into(),
            StatusCode::BAD_REQUEST,
        ),
        (
            format!("/api/v1/artist-identity-groups?q={}", "x".repeat(513)),
            StatusCode::UNPROCESSABLE_ENTITY,
        ),
        (
            format!("{detail_path}?recording_offset=invalid"),
            StatusCode::BAD_REQUEST,
        ),
        (
            format!("{detail_path}?library_offset=0&release_offset=invalid"),
            StatusCode::BAD_REQUEST,
        ),
    ] {
        assert_eq!(
            library
                .owner
                .request(&library.app, "GET", &route, Value::Null)
                .await
                .0,
            expected
        );
    }
    let bounded = library
        .owner
        .get(
            &library.app,
            "/api/v1/artist-identity-groups?offset=-3&limit=1000",
        )
        .await;
    assert_eq!(bounded["offset"], 0);
    assert_eq!(bounded["limit"], 200);
    assert_eq!(bounded["total"], 1);
    let paged = library
        .owner
        .get(
            &library.app,
            &format!("{detail_path}?library_offset=1&recording_offset=0&release_offset=2&limit=1"),
        )
        .await;
    for (page, offset, length) in [
        ("libraries", 1, 1),
        ("recordings", 0, 1),
        ("releases", 2, 0),
    ] {
        assert_eq!(paged[page]["total"], 2);
        assert_eq!(paged[page]["offset"], offset);
        assert_eq!(paged[page]["limit"], 1);
        assert_eq!(paged[page]["items"].as_array().unwrap().len(), length);
    }
    let hidden = library
        .owner
        .request(
            &library.app,
            "GET",
            &format!("{detail_path}?library={}", Uuid::new_v4()),
            Value::Null,
        )
        .await;
    assert_eq!(hidden.0, StatusCode::NOT_FOUND);
    let mut attempts = Vec::new();
    for (pointer, value) in [
        ("/request_id", json!("invalid UUID")),
        ("/request_id", json!(Uuid::nil())),
        ("/expected_revision", json!("invalid revision")),
        ("/expected_revision", json!(-1)),
        ("/association_id", json!("invalid UUID")),
        ("/context", Value::Null),
    ] {
        let mut body = choose.clone();
        *body.pointer_mut(pointer).unwrap() = value;
        attempts.push(("PUT", path.clone(), body));
    }
    let mut unknown = choose.clone();
    unknown["unexpected"] = json!("private-identity-body-canary");
    attempts.push(("PUT", path.clone(), unknown));
    let mut nested_unknown = choose.clone();
    nested_unknown["context"]["unexpected"] = json!("private-identity-body-canary");
    attempts.push(("PUT", path.clone(), nested_unknown));
    for body in [
        json!({"request_id": Uuid::new_v4()}),
        json!({"request_id": Uuid::new_v4(), "expected_revision": "invalid revision"}),
        json!({"request_id": Uuid::nil(), "expected_revision": 1}),
        json!({"request_id": Uuid::new_v4(), "expected_revision": 1, "unexpected": "private-identity-body-canary"}),
    ] {
        attempts.push(("POST", reset_path.clone(), body));
    }
    for (method, route, body) in attempts {
        let (status, error) = library
            .owner
            .request(&library.app, method, &route, body)
            .await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
        assert!(!error.to_string().contains("private-identity-body-canary"));
    }
    let oversized = serde_json::to_vec(
        &json!({"private-padding": "private-identity-body-canary".repeat(11_000)}),
    )
    .unwrap();
    assert!(oversized.len() > 262_144);
    for (method, route, body) in [("PUT", &path, choose), ("POST", &reset_path, reset)] {
        let mut wrong_type = library.owner.headers();
        wrong_type.insert("content-type", "text/plain".parse().unwrap());
        for (headers, bytes, expected) in [
            (
                library.owner.headers(),
                b"{".to_vec(),
                StatusCode::BAD_REQUEST,
            ),
            (
                wrong_type,
                serde_json::to_vec(&body).unwrap(),
                StatusCode::UNSUPPORTED_MEDIA_TYPE,
            ),
            (
                library.owner.headers(),
                oversized.clone(),
                StatusCode::PAYLOAD_TOO_LARGE,
            ),
        ] {
            assert_eq!(raw(library, method, route, headers, bytes).await, expected);
        }
        let mut foreign = library.owner.headers();
        foreign.insert("origin", "https://untrusted.invalid".parse().unwrap());
        let mut no_guard = library.owner.headers();
        no_guard.remove("x-hito-request");
        for headers in [foreign, no_guard] {
            assert_eq!(
                exchange(&library.app, method, route, body.clone(), headers)
                    .await
                    .0,
                StatusCode::FORBIDDEN
            );
        }
    }
    assert_eq!(library.owner.get(&library.app, &path).await, choice_before);
    assert_eq!(
        library.owner.get(&library.app, &detail_path).await,
        detail_before
    );
    assert_eq!(library.snapshot().await, before);
    assert_eq!(library.provider_counts().await, provider_before);
}

async fn raw(
    library: &Library,
    method: &str,
    path: &str,
    headers: HeaderMap,
    body: Vec<u8>,
) -> StatusCode {
    let mut request = Request::builder()
        .method(method)
        .uri(path)
        .body(Body::from(body))
        .unwrap();
    *request.headers_mut() = headers;
    let response =
        tokio::time::timeout(Duration::from_secs(5), library.app.clone().oneshot(request))
            .await
            .unwrap()
            .unwrap();
    Uuid::parse_str(response.headers()["x-request-id"].to_str().unwrap()).unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 8192).await.unwrap();
    let error: Value = serde_json::from_slice(&bytes).unwrap();
    assert!(error["error"].is_string());
    assert!(!error.to_string().contains("private-identity-body-canary"));
    status
}
