#[path = "../../hito-core/tests/fixtures/podcast_audio_service.rs"]
#[allow(dead_code)]
mod fixture;
#[path = "api/support.rs"]
#[allow(dead_code)]
mod support;

use axum::{
    Router,
    body::Body,
    http::{HeaderMap, Request, StatusCode},
};
use fixture::PodcastAudioServer;
use futures_util::FutureExt;
use hito_core::{CoreConfig, HostDeviceId, PodcastNetworkConfig, Services};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use std::{panic::AssertUnwindSafe, path::Path, time::Duration};
use support::call;
use tower::ServiceExt;
use uuid::Uuid;

const PROGRESS: &str = "/api/v1/recordings/progress";

#[tokio::test]
async fn podcast_pages_expose_private_subscriptions_episodes_and_saved_resume() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::create_dir(directory.path().join("music")).unwrap();
    let wav = support::write_wav(&directory.path().join("owned-page.wav"));
    let mut fixture = PodcastAudioServer::start(wav).await;
    let outcome = tokio::time::timeout(
        Duration::from_secs(20),
        AssertUnwindSafe(exercise(directory.path(), &fixture)).catch_unwind(),
    )
    .await;
    assert!(fixture.finish().await, "the owned feed fixture must join");
    match outcome {
        Ok(Ok(())) => {}
        Ok(Err(panic)) => std::panic::resume_unwind(panic),
        Err(_) => panic!("podcast page requests must finish within the deadline"),
    }
}

async fn exercise(directory: &Path, fixture: &PodcastAudioServer) {
    let mut config = CoreConfig::new(
        directory.join("hito.db"),
        directory.join("music"),
        HostDeviceId::new("podcast-page-host").unwrap(),
    );
    config.podcast_network = PodcastNetworkConfig::new(vec![fixture.origin.clone()]).unwrap();
    let services = hito_core::open(config).await.unwrap();
    let app = hito_server::router(services.clone());
    let (status, session) = call(
        &app,
        "POST",
        "/api/v1/setup",
        json!({"username": "podcast-page-owner", "password": "owned podcast page password"}),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let token = session["token"].as_str().unwrap();
    let (status, receipt) = call(
        &app,
        "POST",
        "/api/v1/podcasts",
        json!({"request_id": Uuid::new_v4(), "feed_url": fixture.feed_url}),
        Some(token),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED);
    let subscription = receipt["subscription_id"].as_str().unwrap();
    let detail_path = format!("/podcasts/{subscription}");
    let pending = page(&app, "/podcasts", Some(token), None, true).await;
    assert_eq!(pending["component"], "PodcastIndexPage");
    assert_eq!(
        pending["props"]["subscriptions"][0]["refresh_pending"],
        true
    );
    assert_eq!(
        pending["props"]["subscriptions"][0]["refresh_failed"],
        false
    );
    assert!(pending["props"]["subscriptions"][0]["last_refreshed_at"].is_null());
    assert_eq!(pending["props"]["subscriptions"][0]["content_revision"], 0);
    publish(&services).await;

    let initial = page(&app, "/podcasts", Some(token), None, false).await;
    assert_eq!(initial["component"], "PodcastIndexPage");
    assert_eq!(initial["clearHistory"], false);
    let subscriptions = &initial["props"]["subscriptions"];
    assert_eq!(subscriptions.as_array().unwrap().len(), 1);
    assert_eq!(subscriptions[0]["subscription_id"], subscription);
    assert_eq!(subscriptions[0]["title"], "Owned audio feed");
    assert_eq!(subscriptions[0]["refresh_pending"], false);
    assert_eq!(subscriptions[0]["refresh_failed"], false);
    assert!(subscriptions[0]["last_refreshed_at"].is_i64());
    assert_eq!(subscriptions[0]["content_revision"], 1);
    let detail = page(&app, &detail_path, Some(token), None, true).await;
    assert_eq!(detail["component"], "PodcastPage");
    assert_eq!(detail["props"]["subscription"], subscriptions[0]);
    assert_eq!(detail["props"]["episodes"]["total"], 1);
    assert_eq!(detail["props"]["episodes"]["content_revision"], 1);
    assert_eq!(detail["props"]["progress"], json!([]));
    let recording = detail["props"]["episodes"]["items"][0]["recording"]["id"]
        .as_str()
        .unwrap();
    let batch = json!({"device_id": "podcast-page-client", "operations": [{
        "id": Uuid::new_v4(),
        "action": {"kind": "progress_set", "recording_id": recording, "position_ms": 700, "speed": 1.25}
    }]});
    assert_eq!(
        call(&app, "POST", "/api/v1/sync", batch, Some(token))
            .await
            .0,
        StatusCode::OK
    );
    let expected = json!([{"recording_id": recording, "position_ms": 700, "speed": 1.25}]);
    let resumed = page(&app, &detail_path, Some(token), Some("progress"), true).await;
    assert_eq!(
        resumed["props"],
        json!({"errors": {}, "progress": expected})
    );
    let episodes = page(&app, &detail_path, Some(token), Some("episodes"), true).await;
    assert_eq!(
        episodes["props"],
        json!({"errors": {}, "episodes": detail["props"]["episodes"]})
    );
    let index = page(&app, "/podcasts", Some(token), Some("subscriptions"), true).await;
    assert_eq!(
        index["props"],
        json!({"errors": {}, "subscriptions": subscriptions})
    );
    let shared = page(&app, &detail_path, Some(token), Some("user"), true).await;
    assert_eq!(shared["props"].as_object().unwrap().len(), 2);
    assert_eq!(
        shared["props"]["user"]["id"],
        initial["props"]["user"]["id"]
    );
    let (status, saved) = call(
        &app,
        "POST",
        PROGRESS,
        json!({"recording_ids": [recording, recording]}),
        Some(token),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(saved, expected);
    assert_eq!(
        call(
            &app,
            "POST",
            PROGRESS,
            json!({"recording_ids": []}),
            Some(token)
        )
        .await,
        (StatusCode::OK, json!([]))
    );
    for public in [
        initial.to_string(),
        detail.to_string(),
        resumed.to_string(),
        saved.to_string(),
    ] {
        assert!(!public.contains(&fixture.feed_url));
        assert!(!public.contains(&fixture.enclosure_url));
        assert!(!public.contains("podcast-private-enclosure-canary"));
    }

    let credentials =
        json!({"username": "podcast-page-other", "password": "other podcast page password"});
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/v1/users",
            credentials.clone(),
            Some(token)
        )
        .await
        .0,
        StatusCode::CREATED
    );
    let (status, other_session) = call(&app, "POST", "/api/v1/login", credentials, None).await;
    assert_eq!(status, StatusCode::OK);
    let other = other_session["token"].as_str().unwrap();
    assert_eq!(
        page(&app, "/podcasts", Some(other), None, true).await["props"]["subscriptions"],
        json!([])
    );
    for path in ["/podcasts", &detail_path] {
        let anonymous = page(&app, path, None, None, true).await;
        assert_eq!(anonymous["component"], "LoginPage");
        assert_eq!(anonymous["clearHistory"], true);
        assert!(anonymous["props"].get("subscriptions").is_none());
        assert!(anonymous["props"].get("episodes").is_none());
        assert!(anonymous["props"].get("progress").is_none());
    }
    for (path, reader) in [
        (&detail_path, other),
        (&format!("/podcasts/{}", Uuid::new_v4()), token),
    ] {
        for fields in [None, Some("progress"), Some("user")] {
            assert_eq!(
                page_response(&app, path, Some(reader), fields, true)
                    .await
                    .0,
                StatusCode::NOT_FOUND
            );
        }
    }
    assert_eq!(
        call(
            &app,
            "POST",
            PROGRESS,
            json!({"recording_ids": [recording]}),
            Some(other)
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        call(
            &app,
            "POST",
            PROGRESS,
            json!({"recording_ids": [recording, Uuid::new_v4().to_string()]}),
            Some(token)
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        call(
            &app,
            "POST",
            PROGRESS,
            json!({"recording_ids": [recording]}),
            None
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
    strict_progress(&app, token, recording).await;
    let (status, contract) = call(&app, "GET", "/api/v1/openapi.json", Value::Null, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_contract(&contract);
    assert_eq!(fixture.feed_requests(), 1);
    assert_eq!(fixture.enclosure_requests(), 0);
    assert_eq!(fixture.unexpected(), 0);
}

async fn publish(services: &Services) {
    assert!(
        services
            .podcasts
            .run_next_refresh_until(std::future::pending())
            .await
            .unwrap()
    );
}

async fn page(
    app: &Router,
    path: &str,
    token: Option<&str>,
    fields: Option<&str>,
    inertia: bool,
) -> Value {
    let (status, headers, body) = page_response(app, path, token, fields, inertia).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(headers["vary"], "X-Inertia");
    if inertia {
        assert_eq!(headers["x-inertia"], "true");
        serde_json::from_str(&body).unwrap()
    } else {
        assert!(
            headers["content-type"]
                .to_str()
                .unwrap()
                .starts_with("text/html")
        );
        let encoded = body
            .split("<script data-page=\"app\" type=\"application/json\">")
            .nth(1)
            .unwrap()
            .split("</script>")
            .next()
            .unwrap();
        serde_json::from_str(encoded).unwrap()
    }
}

async fn page_response(
    app: &Router,
    path: &str,
    token: Option<&str>,
    fields: Option<&str>,
    inertia: bool,
) -> (StatusCode, HeaderMap, String) {
    let mut request = Request::builder().uri(path);
    if let Some(token) = token {
        request = request.header("cookie", format!("hito_session={token}"));
    }
    if inertia {
        request = request.header("x-inertia", "true");
    }
    if let Some(fields) = fields {
        request = request
            .header(
                "x-inertia-partial-component",
                if path == "/podcasts" {
                    "PodcastIndexPage"
                } else {
                    "PodcastPage"
                },
            )
            .header("x-inertia-partial-data", fields);
    }
    let response = app
        .clone()
        .oneshot(request.body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let headers = response.headers().clone();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (status, headers, String::from_utf8(bytes.to_vec()).unwrap())
}

async fn strict_progress(app: &Router, token: &str, recording: &str) {
    for (body, content_type, expected) in [
        (
            json!({"recording_ids": [recording], "unexpected": "private-input"}).to_string(),
            "application/json",
            StatusCode::UNPROCESSABLE_ENTITY,
        ),
        (
            json!({"recording_ids": ["private-invalid-uuid"]}).to_string(),
            "application/json",
            StatusCode::UNPROCESSABLE_ENTITY,
        ),
        (
            json!({"recording_ids": vec![recording; 201]}).to_string(),
            "application/json",
            StatusCode::UNPROCESSABLE_ENTITY,
        ),
        ("{".to_owned(), "application/json", StatusCode::BAD_REQUEST),
        (
            json!({"recording_ids": [recording]}).to_string(),
            "text/plain",
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
        ),
    ] {
        let request = Request::builder()
            .method("POST")
            .uri(PROGRESS)
            .header("content-type", content_type)
            .header("authorization", format!("Bearer {token}"))
            .header("x-hito-request", "1")
            .body(Body::from(body))
            .unwrap();
        let response = app.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), expected);
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let error = String::from_utf8(bytes.to_vec()).unwrap();
        assert!(!error.contains("private-input"));
        assert!(!error.contains("private-invalid-uuid"));
    }
    let request = Request::builder()
        .method("POST")
        .uri(PROGRESS)
        .header("content-type", "application/json")
        .header("cookie", format!("hito_session={token}"))
        .body(Body::from(
            json!({"recording_ids": [recording]}).to_string(),
        ))
        .unwrap();
    assert_eq!(
        app.clone().oneshot(request).await.unwrap().status(),
        StatusCode::FORBIDDEN
    );
}

fn assert_contract(contract: &Value) {
    let operation = &contract["paths"][PROGRESS]["post"];
    assert_eq!(operation["operationId"], "saved_playback");
    assert_eq!(
        operation["requestBody"]["content"]["application/json"]["schema"],
        json!({"$ref": "#/components/schemas/SavedPlaybackRequest"})
    );
    assert_eq!(
        operation["responses"]["200"]["content"]["application/json"]["schema"],
        json!({"type": "array", "items": {"$ref": "#/components/schemas/RecordingProgress"}})
    );
    for code in ["400", "401", "403", "404", "413", "415", "422", "500"] {
        assert!(operation["responses"][code].is_object());
    }
    for kind in ["session", "bearer"] {
        assert!(
            operation["security"]
                .as_array()
                .unwrap()
                .iter()
                .any(|security| security.get(kind).is_some())
        );
    }
    let schemas = &contract["components"]["schemas"];
    assert_eq!(
        schemas["SavedPlaybackRequest"]["additionalProperties"],
        false
    );
    assert_eq!(
        schemas["SavedPlaybackRequest"]["properties"]["recording_ids"]["items"]["format"],
        "uuid"
    );
    for (name, properties) in [
        ("PodcastIndexPageProps", vec!["subscriptions"]),
        (
            "PodcastPageProps",
            vec!["subscription", "episodes", "progress"],
        ),
    ] {
        let schema = &schemas[name];
        for field in properties {
            let (present, required) = schema_field(contract, schema, field);
            assert!(present, "{name} must expose {field}");
            assert!(required, "{name} must require {field}");
        }
    }
}

fn schema_field(contract: &Value, schema: &Value, field: &str) -> (bool, bool) {
    let mut present = schema["properties"].get(field).is_some();
    let mut required = schema["required"]
        .as_array()
        .is_some_and(|fields| fields.iter().any(|value| value == field));
    if let Some(reference) = schema["$ref"].as_str() {
        let pointer = reference
            .strip_prefix('#')
            .expect("contract references must belong to the served document");
        let referenced = contract
            .pointer(pointer)
            .expect("contract references must resolve");
        let (referenced_present, referenced_required) = schema_field(contract, referenced, field);
        present |= referenced_present;
        required |= referenced_required;
    }
    for part in schema["allOf"].as_array().into_iter().flatten() {
        let (part_present, part_required) = schema_field(contract, part, field);
        present |= part_present;
        required |= part_required;
    }
    (present, required)
}
