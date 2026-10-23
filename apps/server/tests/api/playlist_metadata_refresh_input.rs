use super::{private_copy, request};
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;
use uuid::Uuid;

#[tokio::test]
async fn invalid_refresh_input_and_cookie_origins_cannot_publish_metadata() {
    let f = private_copy().await;
    f.publish_verified_album().await;
    let login = f
        .app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/login")
                .header("content-type", "application/json")
                .header("x-hito-request", "1")
                .body(Body::from(
                    json!({"username": "admin", "password": "administrator test password"})
                        .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(login.status(), StatusCode::OK);
    let cookie = login.headers()["set-cookie"]
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_owned();
    let valid = request(&f);
    let path = format!("/api/v1/playlists/{}/metadata/refresh", f.playlist);
    let before = f.state().await;
    let observed = f.provider_requests().await;
    let mut unknown = valid.clone();
    unknown["unexpected"] = json!("private unexpected input");
    let mut invalid_id = valid.clone();
    invalid_id["request_id"] = json!("private invalid UUID");
    let mut nil_id = valid.clone();
    nil_id["request_id"] = json!(Uuid::nil());
    let mut zero_revision = valid.clone();
    zero_revision["installation_revision"] = json!(0);
    let mut negative_revision = valid.clone();
    negative_revision["installation_revision"] = json!(-1);
    let mut oversized = valid.clone();
    oversized["unexpected"] = json!("x".repeat(262_144));
    let data_error = "Request JSON does not match the expected fields";
    let domain_error = "Use a nonempty request ID and positive connection revision";
    let mut attempts = Vec::new();
    for (name, body, content_type, expected, message) in [
        (
            "malformed JSON",
            "{".to_owned(),
            Some("application/json"),
            StatusCode::BAD_REQUEST,
            "Request body is not valid JSON",
        ),
        (
            "empty JSON",
            String::new(),
            Some("application/json"),
            StatusCode::BAD_REQUEST,
            "Request body is not valid JSON",
        ),
        (
            "null JSON",
            "null".to_owned(),
            Some("application/json"),
            StatusCode::UNPROCESSABLE_ENTITY,
            data_error,
        ),
        (
            "unknown field",
            unknown.to_string(),
            Some("application/json"),
            StatusCode::UNPROCESSABLE_ENTITY,
            data_error,
        ),
        (
            "invalid request UUID",
            invalid_id.to_string(),
            Some("application/json"),
            StatusCode::UNPROCESSABLE_ENTITY,
            data_error,
        ),
        (
            "missing connection revision",
            json!({"request_id": valid["request_id"]}).to_string(),
            Some("application/json"),
            StatusCode::UNPROCESSABLE_ENTITY,
            data_error,
        ),
        (
            "nil request UUID",
            nil_id.to_string(),
            Some("application/json"),
            StatusCode::UNPROCESSABLE_ENTITY,
            domain_error,
        ),
        (
            "zero connection revision",
            zero_revision.to_string(),
            Some("application/json"),
            StatusCode::UNPROCESSABLE_ENTITY,
            domain_error,
        ),
        (
            "negative connection revision",
            negative_revision.to_string(),
            Some("application/json"),
            StatusCode::UNPROCESSABLE_ENTITY,
            domain_error,
        ),
        (
            "oversized JSON",
            oversized.to_string(),
            Some("application/json"),
            StatusCode::PAYLOAD_TOO_LARGE,
            "Request body could not be read",
        ),
        (
            "non-JSON content type",
            valid.to_string(),
            Some("text/plain"),
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "Use the application/json content type",
        ),
        (
            "missing content type",
            valid.to_string(),
            None,
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "Use the application/json content type",
        ),
    ] {
        let mut attempt = Request::builder()
            .method("POST")
            .uri(&path)
            .header("cookie", &cookie)
            .header("x-hito-request", "1");
        if let Some(content_type) = content_type {
            attempt = attempt.header("content-type", content_type);
        }
        attempts.push((
            name,
            attempt.body(Body::from(body)).unwrap(),
            expected,
            message,
        ));
    }
    for (name, guard, origin, body) in [
        ("missing mutation guard", None, None, valid.to_string()),
        (
            "wrong mutation guard",
            Some("0"),
            Some("http://localhost:8080"),
            valid.to_string(),
        ),
        (
            "foreign origin before malformed JSON",
            Some("1"),
            Some("https://untrusted.example"),
            "{".to_owned(),
        ),
        ("opaque origin", Some("1"), Some("null"), valid.to_string()),
    ] {
        let mut attempt = Request::builder()
            .method("POST")
            .uri(&path)
            .header("cookie", &cookie)
            .header("content-type", "application/json");
        if let Some(guard) = guard {
            attempt = attempt.header("x-hito-request", guard);
        }
        if let Some(origin) = origin {
            attempt = attempt.header("origin", origin);
        }
        attempts.push((
            name,
            attempt.body(Body::from(body)).unwrap(),
            StatusCode::FORBIDDEN,
            "Request origin could not be verified",
        ));
    }
    for (name, attempt, expected, message) in attempts {
        let response = f.app.clone().oneshot(attempt).await.unwrap();
        assert_eq!(response.status(), expected, "{name}");
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let error: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(error, json!({"error": message}), "{name}");
        assert_eq!(f.state().await, before, "{name}");
        assert_eq!(f.provider_requests().await, observed, "{name}");
    }
    assert_eq!(
        crate::support::call(
            &f.app,
            "GET",
            &format!("/api/v1/recordings/{}/download", f.recording),
            Value::Null,
            Some(&f.token)
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );

    let response = f
        .app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(&path)
                .header("cookie", &cookie)
                .header("content-type", "application/json")
                .header("x-hito-request", "1")
                .header("origin", "http://localhost:8080")
                .body(Body::from(valid.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let receipt: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(receipt["request_id"], valid["request_id"]);
    assert_eq!(receipt["updated_recordings"], 1);
    let after = f.state().await;
    assert_eq!(after["albums"]["total"], 1);
    assert_eq!(
        after["metadata"]["title"]["personal"],
        before["metadata"]["title"]["personal"]
    );
    assert_eq!(
        after["metadata"]["artist"]["personal"],
        before["metadata"]["artist"]["personal"]
    );
    let mut expected = observed;
    expected[1] += 2;
    expected[2] += 1;
    assert_eq!(f.provider_requests().await, expected);
}
