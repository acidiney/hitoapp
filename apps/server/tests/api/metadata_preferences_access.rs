use crate::metadata_preferences_fixture as fixture;
use hito_core::test_support::metadata_tag_logs as logs;

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use fixture::{CatalogServer, Counts, Library};
use futures_util::FutureExt;
use serde_json::{Value, json};
use std::panic::{AssertUnwindSafe, resume_unwind};
use tower::ServiceExt;
use tracing::instrument::WithSubscriber;
use uuid::Uuid;

const SETTINGS: &str = "/api/v1/metadata/settings";

#[tokio::test]
async fn a_stale_metadata_revision_cannot_replace_accepted_personal_preferences() {
    let library = Library::prepare().await;
    let mut server = CatalogServer::start().await;
    let result = AssertUnwindSafe(stale_revision(&library, &server))
        .catch_unwind()
        .await;
    server.release_profile().await;
    let joined = server.finish().await;
    if let Err(panic) = result {
        resume_unwind(panic);
    }
    assert!(joined, "The owned catalog fixture must join shutdown");
}

async fn stale_revision(library: &Library, server: &CatalogServer) {
    let app = &library.app;
    let account = &library.accounts[0];
    let other = &library.accounts[1];
    let (service, installation) = connect_catalog(library, server).await;
    let before = library.state().await;
    let other_settings = other.get(app, SETTINGS).await;
    let plugins = account.get(app, "/api/v1/plugins").await;
    let selected = provider(&installation);
    let (status, accepted) = account
        .request(
            app,
            "PUT",
            SETTINGS,
            json!({"revision": 0, "enabled": true, "providers": [selected]}),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let captured = logs::CapturedLogs::default();
    let (status, _) = account
        .request(
            app,
            "PUT",
            SETTINGS,
            json!({"revision": 0, "enabled": false, "providers": []}),
        )
        .with_subscriber(captured.subscriber())
        .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert!(
        account.get(app, SETTINGS).await == accepted,
        "A stale revision must retain the exact accepted settings and revision"
    );
    assert!(other.get(app, SETTINGS).await == other_settings);
    assert!(account.get(app, "/api/v1/plugins").await == plugins);
    assert!(
        library.state().await == before,
        "A rejected metadata update must retain all private library and collection state"
    );
    assert_eq!(
        server.counts().await,
        Counts {
            manifests: 1,
            descriptors: 0,
            profiles: 0,
            unexpected: 0
        }
    );
    assert_safe_conflict_log(
        &captured.text(),
        &[
            &account.token,
            &account.id,
            &other.token,
            &other.id,
            &server.url,
            service["id"].as_str().unwrap(),
            installation["id"].as_str().unwrap(),
            "Private administrator credit",
            "Private listener credit",
            "preferences-listener",
            "owned preferences listener password",
            "Fixture.wav",
            "Bearer ",
        ],
    );
}

#[tokio::test]
async fn metadata_provider_authority_is_personal_and_disabled_references_survive_removal() {
    let library = Library::prepare().await;
    let mut server = CatalogServer::start().await;
    let result = AssertUnwindSafe(provider_authority(&library, &server))
        .catch_unwind()
        .await;
    server.release_profile().await;
    let joined = server.finish().await;
    if let Err(panic) = result {
        resume_unwind(panic);
    }
    assert!(joined, "The owned catalog fixture must join shutdown");
}

async fn provider_authority(library: &Library, server: &CatalogServer) {
    let app = &library.app;
    let account = &library.accounts[0];
    let other = &library.accounts[1];
    let (service, installation) = connect_catalog(library, server).await;
    let (status, other_installation) = other.request(app, "POST", "/api/v1/plugins",
        json!({"service_id": service["id"], "service_revision": service["revision"], "bearer_token": null})).await;
    assert_eq!(status, StatusCode::CREATED);
    assert!(
        other_installation["id"] != installation["id"],
        "Each account must retain its own approved installation of the offered catalog"
    );
    let before = library.state().await;
    let other_default = other.get(app, SETTINGS).await;
    let selected = provider(&installation);
    let (status, accepted) = account
        .request(
            app,
            "PUT",
            SETTINGS,
            json!({"revision": 0, "enabled": true, "providers": [selected]}),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    for candidate in [
        selected.clone(),
        json!({"kind": "http_plugin", "installation_id": Uuid::new_v4()}),
    ] {
        let (status, _) = other
            .request(
                app,
                "PUT",
                SETTINGS,
                json!({"revision": 0, "enabled": true, "providers": [candidate]}),
            )
            .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert!(
            other.get(app, SETTINGS).await == other_default,
            "Another account's or absent installation must not change personal provider settings"
        );
    }
    let (status, other_accepted) = other
        .request(
            app,
            "PUT",
            SETTINGS,
            json!({"revision": 0, "enabled": true, "providers": [provider(&other_installation)]}),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        other_accepted
            == json!({"revision": 1, "enabled": true, "providers": [provider(&other_installation)],
                "automatic_artist_lookup": true, "automatic_artwork": true,
                "automatic_service_search": true}),
        "The other account must be able to select only its own approved catalog installation"
    );
    assert!(account.get(app, SETTINGS).await == accepted);
    let (status, _) = account
        .request(
            app,
            "DELETE",
            &format!("/api/v1/plugins/{}", installation["id"].as_str().unwrap()),
            json!({"revision": installation["revision"]}),
        )
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, disabled) = account
        .request(
            app,
            "PUT",
            SETTINGS,
            json!({"revision": accepted["revision"], "enabled": false, "providers": [selected]}),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        disabled
            == json!({"revision": 2, "enabled": false, "providers": [selected],
                "automatic_artist_lookup": true, "automatic_artwork": true,
                "automatic_service_search": true}),
        "Disabling enrichment must retain the removed provider reference and advance its revision"
    );
    let (status, _) = account
        .request(
            app,
            "PUT",
            SETTINGS,
            json!({"revision": disabled["revision"], "enabled": true, "providers": [selected]}),
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(
        account.get(app, SETTINGS).await == disabled,
        "A removed connection must not reenable enrichment or consume the disabled revision"
    );
    assert!(other.get(app, SETTINGS).await == other_accepted);
    assert!(
        library.state().await == before,
        "Connection authority checks and disabling must retain private library and collection state"
    );
    assert_eq!(
        server.counts().await,
        Counts {
            manifests: 2,
            descriptors: 0,
            profiles: 0,
            unexpected: 0
        }
    );
}

#[tokio::test]
async fn metadata_preferences_require_a_session_matching_account_and_browser_mutation_guards() {
    let library = Library::prepare().await;
    let app = &library.app;
    let account = &library.accounts[0];
    let before = library.state().await;
    let settings = account.get(app, SETTINGS).await;
    for (method, authenticated, origin, guard, requested_account, expected) in [
        (
            "GET",
            false,
            "http://localhost:8080",
            true,
            &account.id,
            StatusCode::UNAUTHORIZED,
        ),
        (
            "PUT",
            false,
            "http://localhost:8080",
            true,
            &account.id,
            StatusCode::UNAUTHORIZED,
        ),
        (
            "PUT",
            true,
            "http://localhost:8080",
            true,
            &library.accounts[1].id,
            StatusCode::FORBIDDEN,
        ),
        (
            "PUT",
            true,
            "https://untrusted.invalid",
            true,
            &account.id,
            StatusCode::FORBIDDEN,
        ),
        (
            "PUT",
            true,
            "http://localhost:8080",
            false,
            &account.id,
            StatusCode::FORBIDDEN,
        ),
    ] {
        let mut request = Request::builder()
            .method(method)
            .uri(SETTINGS)
            .header("origin", origin)
            .header("x-hito-account", requested_account.as_str())
            .header("content-type", "application/json");
        if authenticated {
            request = request.header("cookie", format!("hito_session={}", account.token));
        }
        if guard {
            request = request.header("x-hito-request", "1");
        }
        let body = if method == "GET" {
            Body::empty()
        } else {
            Body::from(json!({"revision": 0, "enabled": false, "providers": []}).to_string())
        };
        let response = app
            .clone()
            .oneshot(request.body(body).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), expected);
    }
    assert!(
        account.get(app, SETTINGS).await == settings,
        "Rejected authentication and browser mutations must leave settings unchanged"
    );
    assert!(library.accounts[1].get(app, SETTINGS).await == settings);
    assert!(
        library.state().await == before,
        "Rejected authentication and browser mutations must leave private state unchanged"
    );
}

async fn connect_catalog(library: &Library, server: &CatalogServer) -> (Value, Value) {
    let account = &library.accounts[0];
    let (status, service) = account
        .request(
            &library.app,
            "POST",
            "/api/v1/plugins/services",
            json!({"name": "Owned metadata authority catalog", "base_url": server.url}),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED);
    let (status, installation) = account.request(&library.app, "POST", "/api/v1/plugins",
        json!({"service_id": service["id"], "service_revision": service["revision"], "bearer_token": null})).await;
    assert_eq!(status, StatusCode::CREATED);
    (service, installation)
}

fn provider(installation: &Value) -> Value {
    json!({"kind": "http_plugin", "installation_id": installation["id"]})
}

fn assert_safe_conflict_log(captured: &str, private: &[&str]) {
    for value in private {
        assert!(
            !captured.contains(value),
            "Metadata preference logs must omit private values"
        );
    }
    let events = captured
        .lines()
        .map(|line| {
            serde_json::from_str::<Value>(line)
                .unwrap_or_else(|_| panic!("Metadata preference logs must use structured JSON"))
        })
        .filter(|event| event["fields"]["message"] == "Metadata settings update completed")
        .collect::<Vec<_>>();
    assert!(
        events.len() == 1,
        "The rejected scoped update must log its completion"
    );
    let event = &events[0];
    assert!(
        event["fields"]["outcome"] == "conflict"
            && event["fields"]["elapsed_ms"].as_u64().is_some()
            && event["span"]["operation"] == "metadata_settings_update",
        "The rejected update must log its safe outcome, timing and operation"
    );
    let request = event["span"]["request_id"]
        .as_str()
        .expect("The rejected update must retain a workflow correlation ID");
    assert!(
        Uuid::parse_str(request).is_ok(),
        "Workflow correlation IDs must be generated identifiers"
    );
}
