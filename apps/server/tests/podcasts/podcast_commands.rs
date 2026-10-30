#[path = "../api/support.rs"]
#[allow(dead_code)]
mod support;

use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode},
};
use hito_core::{
    PodcastEpisodePage, PodcastRefreshReceipt, PodcastSubscription, PodcastSubscriptionReceipt,
    PodcastUnsubscribeReceipt, RefreshPodcast, SubscribePodcast, UnsubscribePodcast,
};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use support::{admin, app, call};
use tower::ServiceExt;
use utoipa::{PartialSchema, ToSchema};
use uuid::Uuid;

#[tokio::test]
async fn podcast_commands_preserve_receipts_and_reject_invalid_or_foreign_requests() {
    let (_directory, app) = app().await;
    let token = admin(&app).await;
    let (status, contract) = call(&app, "GET", "/api/v1/openapi.json", Value::Null, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_contract(&contract);
    let credentials = json!({"username": "other", "password": "other podcast command password"});
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/v1/users",
            credentials.clone(),
            Some(&token)
        )
        .await
        .0,
        StatusCode::CREATED
    );
    let (status, session) = call(&app, "POST", "/api/v1/login", credentials, None).await;
    assert_eq!(status, StatusCode::OK);
    let other = session["token"].as_str().unwrap();
    let subscribe = json!({
        "request_id": Uuid::new_v4(),
        "feed_url": "https://podcasts.example.com/owned-command.xml"
    });
    reject_inputs(&app, &token, "/api/v1/podcasts", &subscribe, None).await;
    let subscribed = accepted(
        &app,
        &token,
        "/api/v1/podcasts",
        &subscribe,
        StatusCode::ACCEPTED,
    )
    .await;
    let subscription = subscribed["subscription_id"].as_str().unwrap();
    let episodes_path = format!("/api/v1/podcasts/{subscription}/episodes");
    let refresh_path = format!("/api/v1/podcasts/{subscription}/refresh");
    let unsubscribe_path = format!("/api/v1/podcasts/{subscription}/unsubscribe");
    let initial = public_state(&app, &token, Some(subscription)).await;
    assert_eq!(initial["subscriptions"].as_array().unwrap().len(), 1);
    assert_eq!(initial["subscriptions"][0]["subscription_id"], subscription);
    assert_eq!(initial["episodes"]["body"]["total"], 0);
    assert_eq!(initial["episodes"]["body"]["items"], json!([]));
    for (suffix, expected) in [
        ("?unexpected=private-query-input", StatusCode::BAD_REQUEST),
        ("?limit=invalid", StatusCode::BAD_REQUEST),
        ("?after=invalid", StatusCode::UNPROCESSABLE_ENTITY),
    ] {
        let (status, error) = call(
            &app,
            "GET",
            &format!("{episodes_path}{suffix}"),
            Value::Null,
            Some(&token),
        )
        .await;
        assert_eq!(status, expected);
        assert!(!error.to_string().contains("private-query-input"));
        assert_eq!(
            public_state(&app, &token, Some(subscription)).await,
            initial
        );
    }
    for path in ["/api/v1/podcasts", &episodes_path] {
        assert_eq!(
            call(&app, "GET", path, Value::Null, None).await.0,
            StatusCode::UNAUTHORIZED
        );
    }
    assert_eq!(get(&app, other, "/api/v1/podcasts").await, json!([]));
    assert_eq!(
        call(&app, "GET", &episodes_path, Value::Null, Some(other))
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    let refresh = json!({"request_id": Uuid::new_v4()});
    let unsubscribe = json!({"request_id": Uuid::new_v4()});
    for (path, valid, action) in [
        (&refresh_path, &refresh, "refresh"),
        (&unsubscribe_path, &unsubscribe, "unsubscribe"),
    ] {
        reject_inputs(&app, &token, path, valid, Some(subscription)).await;
        assert_eq!(
            call(&app, "POST", path, valid.clone(), Some(other)).await.0,
            StatusCode::NOT_FOUND
        );
        assert_eq!(
            call(
                &app,
                "POST",
                &format!("/api/v1/podcasts/not-a-uuid/{action}"),
                valid.clone(),
                Some(&token)
            )
            .await
            .0,
            StatusCode::NOT_FOUND
        );
        assert_eq!(
            public_state(&app, &token, Some(subscription)).await,
            initial
        );
    }
    let refreshed = accepted(&app, &token, &refresh_path, &refresh, StatusCode::ACCEPTED).await;
    assert_eq!(refreshed["subscription_id"], subscribed["subscription_id"]);
    assert_eq!(refreshed["job_id"], subscribed["job_id"]);
    let unsubscribed = accepted(
        &app,
        &token,
        &unsubscribe_path,
        &unsubscribe,
        StatusCode::OK,
    )
    .await;
    assert_eq!(
        unsubscribed["subscription_id"],
        subscribed["subscription_id"]
    );
    let inactive = public_state(&app, &token, Some(subscription)).await;
    assert_eq!(inactive["subscriptions"], json!([]));
    assert_eq!(
        inactive["episodes"]["status"],
        StatusCode::NOT_FOUND.as_u16()
    );
    for (path, request, expected, receipt) in [
        (
            "/api/v1/podcasts",
            &subscribe,
            StatusCode::ACCEPTED,
            &subscribed,
        ),
        (&refresh_path, &refresh, StatusCode::ACCEPTED, &refreshed),
        (
            &unsubscribe_path,
            &unsubscribe,
            StatusCode::OK,
            &unsubscribed,
        ),
    ] {
        assert_eq!(
            accepted(&app, &token, path, request, expected).await,
            *receipt
        );
        assert_eq!(
            public_state(&app, &token, Some(subscription)).await,
            inactive
        );
    }
    let mut reactivation = subscribe.clone();
    reactivation["request_id"] = json!(Uuid::new_v4());
    let reactivated = accepted(
        &app,
        &token,
        "/api/v1/podcasts",
        &reactivation,
        StatusCode::ACCEPTED,
    )
    .await;
    assert_eq!(
        reactivated["subscription_id"],
        subscribed["subscription_id"]
    );
    assert_ne!(reactivated["job_id"], subscribed["job_id"]);
    let active = public_state(&app, &token, Some(subscription)).await;
    assert_eq!(active, initial);
    for (path, request, expected, receipt) in [
        (
            "/api/v1/podcasts",
            &subscribe,
            StatusCode::ACCEPTED,
            &subscribed,
        ),
        (&refresh_path, &refresh, StatusCode::ACCEPTED, &refreshed),
        (
            &unsubscribe_path,
            &unsubscribe,
            StatusCode::OK,
            &unsubscribed,
        ),
        (
            "/api/v1/podcasts",
            &reactivation,
            StatusCode::ACCEPTED,
            &reactivated,
        ),
    ] {
        assert_eq!(
            accepted(&app, &token, path, request, expected).await,
            *receipt
        );
        assert_eq!(public_state(&app, &token, Some(subscription)).await, active);
    }
    let current = accepted(
        &app,
        &token,
        &refresh_path,
        &json!({"request_id": Uuid::new_v4()}),
        StatusCode::ACCEPTED,
    )
    .await;
    assert_eq!(current["job_id"], reactivated["job_id"]);
    assert_eq!(get(&app, other, "/api/v1/podcasts").await, json!([]));
}

async fn reject_inputs(
    app: &Router,
    token: &str,
    path: &str,
    valid: &Value,
    subscription: Option<&str>,
) {
    let before = public_state(app, token, subscription).await;
    let mut unknown = valid.clone();
    unknown["unexpected"] = json!("private-command-input");
    let mut invalid_id = valid.clone();
    invalid_id["request_id"] = json!("private-invalid-uuid");
    let body = valid.to_string();
    for (body, content_type, expected, message) in [
        (
            unknown.to_string(),
            Some("application/json"),
            StatusCode::UNPROCESSABLE_ENTITY,
            "Request JSON does not match the expected fields",
        ),
        (
            invalid_id.to_string(),
            Some("application/json"),
            StatusCode::UNPROCESSABLE_ENTITY,
            "Request JSON does not match the expected fields",
        ),
        (
            body[..body.len() - 1].to_owned(),
            Some("application/json"),
            StatusCode::BAD_REQUEST,
            "Request body is not valid JSON",
        ),
        (
            body.clone(),
            Some("text/plain"),
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "Use the application/json content type",
        ),
        (
            body.clone(),
            None,
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "Use the application/json content type",
        ),
    ] {
        let mut request = Request::builder()
            .method("POST")
            .uri(path)
            .header("cookie", format!("hito_session={token}"))
            .header("x-hito-request", "1");
        if let Some(content_type) = content_type {
            request = request.header("content-type", content_type);
        }
        let (status, error) = send(app, request.body(Body::from(body)).unwrap()).await;
        assert_eq!(status, expected);
        assert_eq!(error, json!({"error": message}));
        assert_eq!(public_state(app, token, subscription).await, before);
    }
    for foreign_origin in [false, true] {
        let mut request = Request::builder()
            .method("POST")
            .uri(path)
            .header("cookie", format!("hito_session={token}"))
            .header("content-type", "application/json");
        if foreign_origin {
            request = request
                .header("x-hito-request", "1")
                .header("origin", "https://untrusted.example");
        }
        let (status, error) = send(app, request.body(Body::from(body.clone())).unwrap()).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(
            error,
            json!({"error": "Request origin could not be verified"})
        );
        assert_eq!(public_state(app, token, subscription).await, before);
    }
    assert_eq!(
        call(app, "POST", path, valid.clone(), None).await.0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(public_state(app, token, subscription).await, before);
}

async fn accepted(
    app: &Router,
    token: &str,
    path: &str,
    request: &Value,
    expected: StatusCode,
) -> Value {
    let (status, receipt) = call(app, "POST", path, request.clone(), Some(token)).await;
    assert_eq!(status, expected);
    assert_eq!(receipt["request_id"], request["request_id"]);
    assert!(
        !Uuid::parse_str(receipt["subscription_id"].as_str().unwrap())
            .unwrap()
            .is_nil()
    );
    if let Some(job) = receipt.get("job_id") {
        assert!(!Uuid::parse_str(job.as_str().unwrap()).unwrap().is_nil());
    }
    receipt
}

async fn send(app: &Router, request: Request<Body>) -> (StatusCode, Value) {
    let response = app.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (status, serde_json::from_slice(&bytes).unwrap())
}

async fn get(app: &Router, token: &str, path: &str) -> Value {
    let (status, body) = call(app, "GET", path, Value::Null, Some(token)).await;
    assert_eq!(status, StatusCode::OK);
    body
}

async fn public_state(app: &Router, token: &str, subscription: Option<&str>) -> Value {
    let mut state = json!({
        "subscriptions": get(app, token, "/api/v1/podcasts").await,
        "recordings": get(app, token, "/api/v1/recordings").await
    });
    if let Some(subscription) = subscription {
        let (status, body) = call(
            app,
            "GET",
            &format!("/api/v1/podcasts/{subscription}/episodes"),
            Value::Null,
            Some(token),
        )
        .await;
        state["episodes"] = json!({"status": status.as_u16(), "body": body});
    }
    state
}

fn reference<T: ToSchema>() -> Value {
    json!({"$ref": format!("#/components/schemas/{}", T::name())})
}

fn assert_contract(contract: &Value) {
    let paths = &contract["paths"];
    let schemas = &contract["components"]["schemas"];
    let list = &paths["/api/v1/podcasts"]["get"];
    let list_schema = &list["responses"]["200"]["content"]["application/json"]["schema"];
    assert_eq!(list_schema["type"], "array");
    assert_eq!(list_schema["items"], reference::<PodcastSubscription>());
    let episodes = &paths["/api/v1/podcasts/{id}/episodes"]["get"];
    assert_eq!(
        episodes["responses"]["200"]["content"]["application/json"]["schema"],
        reference::<PodcastEpisodePage>()
    );
    for name in ["limit", "after"] {
        assert!(
            episodes["parameters"]
                .as_array()
                .unwrap()
                .iter()
                .any(|parameter| parameter["name"] == name && parameter["in"] == "query")
        );
    }
    for operation in [list, episodes] {
        assert!(
            operation["security"]
                .as_array()
                .unwrap()
                .iter()
                .any(|security| security.get("session").is_some())
        );
        assert!(
            operation["security"]
                .as_array()
                .unwrap()
                .iter()
                .any(|security| security.get("bearer").is_some())
        );
        assert!(operation["responses"]["401"].is_object());
    }
    for (path, request, response, status) in [
        (
            "/api/v1/podcasts",
            reference::<SubscribePodcast>(),
            reference::<PodcastSubscriptionReceipt>(),
            "202",
        ),
        (
            "/api/v1/podcasts/{id}/refresh",
            reference::<RefreshPodcast>(),
            reference::<PodcastRefreshReceipt>(),
            "202",
        ),
        (
            "/api/v1/podcasts/{id}/unsubscribe",
            reference::<UnsubscribePodcast>(),
            reference::<PodcastUnsubscribeReceipt>(),
            "200",
        ),
    ] {
        let operation = &paths[path]["post"];
        assert_eq!(
            operation["requestBody"]["content"]["application/json"]["schema"],
            request
        );
        assert_eq!(
            operation["responses"][status]["content"]["application/json"]["schema"],
            response
        );
        assert!(
            operation["security"]
                .as_array()
                .unwrap()
                .iter()
                .any(|security| security.get("session").is_some())
        );
        assert!(
            operation["security"]
                .as_array()
                .unwrap()
                .iter()
                .any(|security| security.get("bearer").is_some())
        );
        for code in ["400", "401", "403", "404", "409", "413", "415", "422"] {
            assert!(operation["responses"][code].is_object());
        }
    }
    for (name, schema) in [
        (SubscribePodcast::name(), SubscribePodcast::schema()),
        (RefreshPodcast::name(), RefreshPodcast::schema()),
        (UnsubscribePodcast::name(), UnsubscribePodcast::schema()),
    ] {
        assert_eq!(
            schemas[name.as_ref()],
            serde_json::to_value(schema).unwrap()
        );
        assert_eq!(schemas[name.as_ref()]["additionalProperties"], false);
        assert_eq!(
            schemas[name.as_ref()]["properties"]["request_id"]["format"],
            "uuid"
        );
    }
    for (name, schema) in [
        (PodcastSubscription::name(), PodcastSubscription::schema()),
        (
            PodcastSubscriptionReceipt::name(),
            PodcastSubscriptionReceipt::schema(),
        ),
        (PodcastEpisodePage::name(), PodcastEpisodePage::schema()),
        (
            PodcastRefreshReceipt::name(),
            PodcastRefreshReceipt::schema(),
        ),
        (
            PodcastUnsubscribeReceipt::name(),
            PodcastUnsubscribeReceipt::schema(),
        ),
    ] {
        assert_eq!(
            schemas[name.as_ref()],
            serde_json::to_value(schema).unwrap()
        );
    }
}
