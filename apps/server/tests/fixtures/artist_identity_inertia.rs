use crate::artist_review_fixture::Account;
use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use serde_json::Value;
use std::time::Duration;
use tower::ServiceExt;
use uuid::Uuid;

pub(super) async fn page(
    app: &Router,
    account: &Account,
    path: &str,
    partial: Option<(&str, &str)>,
    html: bool,
) -> Value {
    let mut headers = account.headers();
    if !html {
        headers.insert("x-inertia", "true".parse().unwrap());
    }
    if let Some((component, fields)) = partial {
        assert!(!html);
        headers.insert("x-inertia-partial-component", component.parse().unwrap());
        headers.insert("x-inertia-partial-data", fields.parse().unwrap());
    }
    let mut request = Request::builder().uri(path).body(Body::empty()).unwrap();
    *request.headers_mut() = headers;
    let response = tokio::time::timeout(Duration::from_secs(5), app.clone().oneshot(request))
        .await
        .expect("The owned Inertia request must finish within its deadline")
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["cache-control"], "private, no-store");
    assert_eq!(response.headers()["vary"], "X-Inertia");
    assert!(Uuid::parse_str(response.headers()["x-request-id"].to_str().unwrap()).is_ok());
    if !html {
        assert_eq!(response.headers()["x-inertia"], "true");
    }
    let bytes = to_bytes(response.into_body(), 524_288).await.unwrap();
    let value: Value = if html {
        let document = std::str::from_utf8(&bytes).unwrap();
        let embedded = document
            .split_once("<script data-page=\"app\" type=\"application/json\">")
            .unwrap()
            .1
            .split_once("</script>")
            .unwrap()
            .0;
        serde_json::from_str(embedded).unwrap()
    } else {
        serde_json::from_slice(&bytes).unwrap()
    };
    assert_eq!(value["url"], path);
    assert_eq!(value["encryptHistory"], true);
    assert_eq!(value["clearHistory"], false);
    value
}
