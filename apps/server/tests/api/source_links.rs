use crate::support::call;
use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode},
};
use hito_core::test_support::source_link_service::{
    Answer, KNOWN_RECORDING, KNOWN_REFERENCE, SourceLinkServer,
};
use hito_core::{InstallPlugin, OfferPluginService, PluginInstallation, Services};
use hito_plugin_sdk::Capability;
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;

struct Setup {
    hito: Services,
    app: Router,
    token: String,
    recording: String,
    shared: String,
}

#[path = "source_link_fixture.rs"]
mod fixture;

async fn connect(setup: &Setup, server: &SourceLinkServer) -> PluginInstallation {
    install(&setup.hito, &setup.token, server).await
}

async fn install(hito: &Services, token: &str, server: &SourceLinkServer) -> PluginInstallation {
    let actor = hito.identity.authenticate(token).await.unwrap();
    let service = hito
        .plugins
        .offer_service(
            &actor,
            OfferPluginService {
                name: format!("Owned source links {}", uuid::Uuid::new_v4()),
                base_url: server.url.clone(),
            },
        )
        .await
        .unwrap();
    hito.plugins
        .install(
            &actor,
            InstallPlugin {
                accept_manifest_digest: None,
                service_id: service.id,
                service_revision: service.revision,
                bearer_token: None,
            },
        )
        .await
        .unwrap()
}

async fn lookup(
    setup: &Setup,
    token: Option<&str>,
    body: Value,
) -> (StatusCode, Option<String>, String) {
    lookup_on(setup, &setup.recording, token, body).await
}

async fn lookup_on(
    setup: &Setup,
    recording: &str,
    token: Option<&str>,
    body: Value,
) -> (StatusCode, Option<String>, String) {
    let mut request = Request::builder()
        .method("POST")
        .uri(format!("/api/v1/recordings/{recording}/sources/lookup"))
        .header("content-type", "application/json")
        .header("x-hito-request", "1");
    if let Some(token) = token {
        request = request.header("authorization", format!("Bearer {token}"));
    }
    let response = setup
        .app
        .clone()
        .oneshot(request.body(Body::from(body.to_string())).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let cache = response
        .headers()
        .get("cache-control")
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (status, cache, String::from_utf8(bytes.to_vec()).unwrap())
}

fn body(installation: &PluginInstallation, reference: &str) -> Value {
    json!({
        "installation_id": installation.id,
        "installation_revision": installation.revision,
        "reference": reference,
    })
}

async fn sources(setup: &Setup) -> Value {
    call(
        &setup.app,
        "GET",
        &format!("/api/v1/recordings/{}/sources", setup.recording),
        Value::Null,
        Some(&setup.token),
    )
    .await
    .1
}

#[tokio::test]
async fn http_lookup_returns_a_private_candidate_and_attaches_nothing() {
    fixture::run(async |owned| {
        let server = owned.server().await;
        let setup = owned.setup.as_ref().unwrap();
        let installation = connect(setup, &server).await;
        let before = sources(setup).await;

        let (status, cache, text) = lookup(
            setup,
            Some(&setup.token),
            body(&installation, KNOWN_REFERENCE),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{text}");
        assert_eq!(cache.as_deref(), Some("private, no-store"));
        assert_eq!(
            serde_json::from_str::<Value>(&text).unwrap(),
            json!({
                "installation_id": installation.id,
                "installation_revision": installation.revision,
                "provider_recording_id": KNOWN_RECORDING,
            })
        );
        assert_eq!(
            server.lookup_references(),
            [json!({"reference": KNOWN_REFERENCE})]
        );
        assert!(server.lookup_requests()[0].headers.get("cookie").is_none());
        assert_eq!(server.detail_requests(), 0);
        assert_eq!(server.source_requests(), 0);
        assert_eq!(sources(setup).await, before);
        assert_eq!(server.unexpected(), 0);
    })
    .await;
}

#[tokio::test]
async fn http_lookup_requires_the_listener_session_before_any_service_call() {
    fixture::run(async |owned| {
        let server = owned.server().await;
        let setup = owned.setup.as_ref().unwrap();
        let installation = connect(setup, &server).await;
        for token in [None, Some("not-a-session")] {
            let (status, cache, _) = lookup(setup, token, body(&installation, KNOWN_REFERENCE)).await;
            assert_eq!(status, StatusCode::UNAUTHORIZED);
            assert_eq!(cache.as_deref(), Some("private, no-store"));
        }
        assert!(server.lookup_references().is_empty());
    })
    .await;
}

#[tokio::test]
async fn http_lookup_answers_a_foreign_link_with_a_private_not_found() {
    fixture::run(async |owned| {
        let server = owned.server().await;
        let setup = owned.setup.as_ref().unwrap();
        let installation = connect(setup, &server).await;
        let reference = "https://elsewhere.example/owned/private-foreign-sentinel";
        let (status, cache, text) =
            lookup(setup, Some(&setup.token), body(&installation, reference)).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(cache.as_deref(), Some("private, no-store"));
        assert!(!text.contains("private-foreign-sentinel"));
        assert!(!text.contains("elsewhere.example"));
        assert_eq!(server.lookup_references().len(), 1);
    })
    .await;
}

#[tokio::test]
async fn http_lookup_on_a_shared_library_recording_is_not_found_before_any_service_call() {
    fixture::run(async |owned| {
        let server = owned.server().await;
        let setup = owned.setup.as_ref().unwrap();
        let installation = connect(setup, &server).await;
        let (status, cache, text) = lookup_on(
            setup,
            &setup.shared,
            Some(&setup.token),
            body(&installation, KNOWN_REFERENCE),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(cache.as_deref(), Some("private, no-store"));
        assert!(!text.contains(KNOWN_REFERENCE));
        assert!(server.lookup_references().is_empty());
    })
    .await;
}

#[tokio::test]
async fn http_lookup_rejects_malformed_bodies_without_contacting_the_service() {
    fixture::run(async |owned| {
        let server = owned.server().await;
        let setup = owned.setup.as_ref().unwrap();
        let installation = connect(setup, &server).await;
        let mut extra = body(&installation, KNOWN_REFERENCE);
        extra["url"] = json!("https://elsewhere.example/private-extra-sentinel");
        for request in [
            extra,
            json!({"installation_id": installation.id, "reference": KNOWN_REFERENCE}),
            body(&installation, &"a".repeat(2049)),
            body(&installation, "owned\nlink"),
            body(&installation, ""),
        ] {
            let (status, cache, text) = lookup(setup, Some(&setup.token), request).await;
            assert!(status.is_client_error(), "{status}");
            assert_ne!(status, StatusCode::NOT_FOUND);
            assert_eq!(cache.as_deref(), Some("private, no-store"));
            assert!(!text.contains("private-extra-sentinel"));
        }
        assert!(server.lookup_references().is_empty());
    })
    .await;
}

#[tokio::test]
async fn http_lookup_rejects_a_stale_connection_revision() {
    fixture::run(async |owned| {
        let server = owned.server().await;
        let setup = owned.setup.as_ref().unwrap();
        let installation = connect(setup, &server).await;
        let mut stale = body(&installation, KNOWN_REFERENCE);
        stale["installation_revision"] = json!(installation.revision + 1);
        let (status, _, _) = lookup(setup, Some(&setup.token), stale).await;
        assert_eq!(status, StatusCode::CONFLICT);
        assert!(server.lookup_references().is_empty());
    })
    .await;
}

#[tokio::test]
async fn an_upstream_credential_refusal_is_not_a_listener_sign_out() {
    fixture::run(async |owned| {
        let server = owned.server().await;
        let setup = owned.setup.as_ref().unwrap();
        let installation = connect(setup, &server).await;
        server.answer("owned-refused", Answer::Status(StatusCode::UNAUTHORIZED));
        let (status, cache, text) = lookup(
            setup,
            Some(&setup.token),
            body(&installation, "owned-refused"),
        )
        .await;
        assert_eq!(status, StatusCode::CONFLICT);
        let error: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(error["code"], "plugin_credentials_required");
        assert!(error.get("plugin_connection").is_none());
        assert_eq!(cache.as_deref(), Some("private, no-store"));
        assert!(!text.contains("owned-refused"));
        let (status, _) = call(
            &setup.app,
            "GET",
            "/api/v1/recordings",
            Value::Null,
            Some(&setup.token),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
    })
    .await;
}

#[tokio::test]
async fn http_lookup_on_an_old_approval_requires_review_before_any_service_call() {
    fixture::run(async |owned| {
        let server = owned.server_with_capabilities(
            "org.hito.http-upgraded-source-link-fixture",
            [Capability::Search, Capability::Sources],
        )
        .await;
        let setup = owned.setup.as_ref().unwrap();
        let installation = connect(setup, &server).await;
        let (status, cache, _) = lookup(
            setup,
            Some(&setup.token),
            body(&installation, KNOWN_REFERENCE),
        )
        .await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(cache.as_deref(), Some("private, no-store"));
        assert!(server.lookup_references().is_empty());

        server.declare([
            Capability::Search,
            Capability::Sources,
            Capability::SourceLinks,
        ]);
        let refresh = format!("/api/v1/plugins/{}/refresh", installation.id);
        let (status, review) = call(
            &setup.app,
            "POST",
            &refresh,
            json!({"revision": installation.revision, "accept_manifest_digest": null}),
            Some(&setup.token),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(review["kind"], "review_required");
        let (status, _, _) = lookup(
            setup,
            Some(&setup.token),
            body(&installation, KNOWN_REFERENCE),
        )
        .await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
        assert!(
            server.lookup_references().is_empty(),
            "an unaccepted declaration must not reach the service"
        );

        let (status, accepted) = call(
            &setup.app,
            "POST",
            &refresh,
            json!({"revision": installation.revision, "accept_manifest_digest": review["digest"]}),
            Some(&setup.token),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(accepted["kind"], "updated");
        let revision = accepted["installation"]["revision"].as_i64().unwrap();
        let (status, _, text) = lookup(
            setup,
            Some(&setup.token),
            json!({
                "installation_id": installation.id,
                "installation_revision": revision,
                "reference": KNOWN_REFERENCE,
            }),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{text}");
        assert_eq!(server.lookup_references().len(), 1);
    })
    .await;
}

#[path = "source_link_credentials.rs"]
mod credentials;
