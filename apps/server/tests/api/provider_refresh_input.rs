use super::{
    provider_fixture::ProviderServer,
    support::{admin, call, services},
};
use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode},
};
use hito_core::{AdoptProviderRecording, AttachReviewedProviderSource, ProviderCandidate};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;

#[tokio::test]
async fn bodyless_refresh_without_content_type_refreshes_every_binding_manually() {
    assert_manual_request(Body::empty(), None).await;
}

#[tokio::test]
async fn null_refresh_body_refreshes_every_binding_manually() {
    assert_manual_request(Body::from("null"), Some("application/json")).await;
}

#[tokio::test]
async fn empty_refresh_object_refreshes_every_binding_manually() {
    assert_manual_request(Body::from("{}"), Some("application/json")).await;
}

#[tokio::test]
async fn explicit_manual_purpose_refreshes_every_binding() {
    assert_manual_request(
        Body::from(r#"{"purpose":"manual"}"#),
        Some("application/json"),
    )
    .await;
}

#[tokio::test]
async fn unknown_refresh_purpose_is_rejected_before_provider_io() {
    assert_rejected(
        Body::from(r#"{"purpose":"unrecognized-private-value"}"#),
        Some("application/json"),
        StatusCode::UNPROCESSABLE_ENTITY,
        "Request JSON does not match the expected fields",
    )
    .await;
}

#[tokio::test]
async fn unknown_refresh_field_is_rejected_before_provider_io() {
    assert_rejected(
        Body::from(r#"{"purpose":"playback","unexpected":"private-field-value"}"#),
        Some("application/json"),
        StatusCode::UNPROCESSABLE_ENTITY,
        "Request JSON does not match the expected fields",
    )
    .await;
}

#[tokio::test]
async fn malformed_refresh_json_is_rejected_before_provider_io() {
    assert_rejected(
        Body::from(r#"{"purpose":"playback""#),
        Some("application/json"),
        StatusCode::BAD_REQUEST,
        "Request body is not valid JSON",
    )
    .await;
}

#[tokio::test]
async fn non_json_refresh_content_type_is_rejected_before_provider_io() {
    assert_rejected(
        Body::from(r#"{"purpose":"playback"}"#),
        Some("text/plain"),
        StatusCode::UNSUPPORTED_MEDIA_TYPE,
        "Use the application/json content type",
    )
    .await;
}

#[tokio::test]
async fn nonempty_refresh_without_content_type_is_rejected_before_provider_io() {
    assert_rejected(
        Body::from(r#"{"purpose":"playback"}"#),
        None,
        StatusCode::UNSUPPORTED_MEDIA_TYPE,
        "Use the application/json content type",
    )
    .await;
}

#[tokio::test]
async fn oversized_refresh_json_is_rejected_before_provider_io() {
    assert_rejected(
        Body::from(json!({"purpose": "x".repeat(262_144)}).to_string()),
        Some("application/json"),
        StatusCode::PAYLOAD_TOO_LARGE,
        "Request body could not be read",
    )
    .await;
}

async fn assert_manual_request(body: Body, content_type: Option<&str>) {
    let fixture = RefreshFixture::start().await;
    let before = fixture.source_requests().await;
    for (index, provider) in fixture.providers.iter().enumerate() {
        provider
            .reply(
                "v1/sources",
                json!({"items": [{
                    "id": format!("replacement-{index}"),
                    "label": format!("Replacement {index}"), "ready": true
                }]}),
            )
            .await;
    }
    let (status, published) = fixture.request(body, content_type).await;
    assert_eq!(status, StatusCode::OK, "{published}");
    let sources = published.as_array().unwrap();
    assert_eq!(sources.len(), 2);
    assert!(sources.iter().all(|source| source["ready"] == true));
    let accepted = fixture.accepted.as_array().unwrap();
    assert!(
        sources
            .iter()
            .all(|source| source["recording_id"] == accepted[0]["recording_id"])
    );
    assert!(
        sources
            .iter()
            .all(|source| accepted.iter().all(|old| old["id"] != source["id"]))
    );
    assert_eq!(
        fixture.source_requests().await,
        [before[0] + 1, before[1] + 1]
    );
    assert_eq!(fixture.sources().await, published);
}

async fn assert_rejected(
    body: Body,
    content_type: Option<&str>,
    expected: StatusCode,
    message: &str,
) {
    let fixture = RefreshFixture::start().await;
    let before = fixture.provider_requests().await;
    let (status, response) = fixture.request(body, content_type).await;
    assert_eq!(status, expected, "{response}");
    assert_eq!(response, json!({"error": message}));
    assert_eq!(fixture.provider_requests().await, before);
    assert_eq!(fixture.sources().await, fixture.accepted);
}

struct RefreshFixture {
    _directory: tempfile::TempDir,
    app: Router,
    token: String,
    sources_path: String,
    providers: [ProviderServer; 2],
    accepted: Value,
}

impl RefreshFixture {
    async fn start() -> Self {
        let providers = [ProviderServer::start().await, ProviderServer::start().await];
        let directory = tempfile::tempdir().unwrap();
        let hito = services(directory.path(), &directory.path().join("music")).await;
        let app = hito_server::router(hito.clone());
        let token = admin(&app).await;
        let actor = hito.identity.authenticate(&token).await.unwrap();
        let mut installations = Vec::new();
        for (index, provider) in providers.iter().enumerate() {
            provider.reply("v1/recordings/detail", json!({
                "id": format!("item-{index}"), "title": "Owned track", "artist": "Owned artist"
            })).await;
            provider
                .reply(
                    "v1/sources",
                    json!({"items": [{
                        "id": format!("old-{index}"), "label": "Accepted source", "ready": true
                    }]}),
                )
                .await;
            let (status, service) = call(
                &app,
                "POST",
                "/api/v1/plugins/services",
                json!({
                    "name": "Owned input provider", "base_url": provider.url
                }),
                Some(&token),
            )
            .await;
            assert_eq!(status, StatusCode::CREATED);
            let (status, installation) = call(&app, "POST", "/api/v1/plugins", json!({
                "service_id": service["id"], "service_revision": service["revision"], "bearer_token": null
            }), Some(&token)).await;
            assert_eq!(status, StatusCode::CREATED);
            installations.push((
                installation["id"].as_str().unwrap().to_owned(),
                installation["revision"].as_i64().unwrap(),
            ));
        }
        let recording = hito
            .catalog
            .adopt_provider(
                &actor,
                &installations[0].0,
                AdoptProviderRecording {
                    installation_revision: installations[0].1,
                    provider_recording_id: "item-0".into(),
                },
            )
            .await
            .unwrap();
        let candidate = ProviderCandidate {
            installation_id: installations[1].0.clone(),
            installation_revision: installations[1].1,
            provider_recording_id: "item-1".into(),
        };
        let review = hito
            .catalog
            .review_provider_source(&actor, &recording.id, candidate.clone())
            .await
            .unwrap();
        hito.catalog
            .attach_provider_source(
                &actor,
                &recording.id,
                AttachReviewedProviderSource {
                    candidate,
                    review_stamp: review.review_stamp,
                },
            )
            .await
            .unwrap();
        let sources_path = format!("/api/v1/recordings/{}/sources", recording.id);
        let (status, accepted) = call(&app, "GET", &sources_path, Value::Null, Some(&token)).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(accepted.as_array().unwrap().len(), 2);
        Self {
            _directory: directory,
            app,
            token,
            sources_path,
            providers,
            accepted,
        }
    }

    async fn request(&self, body: Body, content_type: Option<&str>) -> (StatusCode, Value) {
        let mut request = Request::builder()
            .method("POST")
            .uri(format!("{}/refresh", self.sources_path))
            .header("x-hito-request", "1")
            .header("authorization", format!("Bearer {}", self.token));
        if let Some(content_type) = content_type {
            request = request.header("content-type", content_type);
        }
        let response = self
            .app
            .clone()
            .oneshot(request.body(body).unwrap())
            .await
            .unwrap();
        let status = response.status();
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        (status, serde_json::from_slice(&bytes).unwrap())
    }

    async fn sources(&self) -> Value {
        let (status, sources) = call(
            &self.app,
            "GET",
            &self.sources_path,
            Value::Null,
            Some(&self.token),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        sources
    }

    async fn source_requests(&self) -> [usize; 2] {
        [
            self.providers[0].requests("v1/sources").await.len(),
            self.providers[1].requests("v1/sources").await.len(),
        ]
    }

    async fn provider_requests(&self) -> Vec<usize> {
        let mut counts = Vec::new();
        for provider in &self.providers {
            for path in [
                "v1/search",
                "v1/recordings/detail",
                "v1/playlists/detail",
                "v1/playlists/items",
                "v1/sources",
                "v1/audio",
            ] {
                counts.push(provider.requests(path).await.len());
            }
        }
        counts
    }
}
