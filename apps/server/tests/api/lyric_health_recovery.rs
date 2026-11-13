use super::support;
use axum::http::StatusCode;
use futures_util::FutureExt;
use hito_plugin_sdk::{
    Capability, HEALTH_PATH, HealthReport, Manifest, PROTOCOL_VERSION, SEARCH_PATH, SearchPage,
};
use hito_testkit::fake_plugin::{FakePlugin, Reply};
use serde_json::{Value, json};
use std::{
    panic::{AssertUnwindSafe, resume_unwind},
    time::{Duration, Instant},
};

use crate::{account, lyric_identity_fixture as library};

const TOKEN: &str = "owned-health-recovery-token";

fn owner_request(request: &hito_testkit::fake_plugin::RecordedRequest) -> bool {
    request
        .headers
        .get("authorization")
        .is_some_and(|value| value == format!("Bearer {TOKEN}").as_str())
}

#[tokio::test]
async fn a_new_protected_success_clears_an_older_health_credential_diagnosis_only_for_its_owner() {
    let mut plugin = FakePlugin::builder(Manifest {
        id: "org.hito.health-recovery-fixture".into(),
        name: "Owned health recovery service".into(),
        protocol_version: PROTOCOL_VERSION,
        capabilities: [Capability::Search, Capability::Metadata, Capability::Lyrics].into(),
        authorization: None,
        content_policy: None,
    })
    .require_protocol()
    .get(HEALTH_PATH, |request| {
        if owner_request(request) {
            Reply::Status(StatusCode::UNAUTHORIZED)
        } else {
            Reply::Json(serde_json::to_value(HealthReport::ok()).unwrap())
        }
    })
    .post(SEARCH_PATH, |request| {
        assert!(owner_request(request));
        Reply::Json(
            serde_json::to_value(SearchPage {
                items: Vec::new(),
                next_cursor: None,
            })
            .unwrap(),
        )
    })
    .start()
    .await;
    let result = AssertUnwindSafe(exercise(&plugin)).catch_unwind().await;
    assert!(plugin.finish().await);
    let installation = match result {
        Ok(installation) => installation,
        Err(panic) => resume_unwind(panic),
    };
    assert_eq!(
        installation.get("health"),
        None,
        "A new checked protected success must clear the older health credential diagnosis"
    );
}

async fn exercise(plugin: &FakePlugin) -> Value {
    let library = library::Library::prepare().await;
    let installation = library
        .account
        .connect(
            &library.app,
            &plugin.url,
            "Owned health recovery service",
            Some(TOKEN),
        )
        .await;
    let (status, headers, settings) = account::exchange(
        &library.app,
        "GET",
        "/api/v1/plugins",
        Value::Null,
        library.account.headers(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(headers["cache-control"], "private, no-store");
    assert_eq!(
        settings["installations"][0]["health"]["state"],
        "credentials_required"
    );
    assert_eq!(plugin.health_requests().len(), 1);
    assert!(owner_request(&plugin.health_requests()[0]));
    assert!(plugin.requests_for(SEARCH_PATH).is_empty());
    let observed_at = Instant::now();
    let service = settings["services"]
        .as_array()
        .unwrap()
        .iter()
        .find(|service| service["id"] == installation["service_id"])
        .unwrap();
    let (status, _) = library.account.request(
        &library.app, "POST", "/api/v1/users",
        json!({"username":"health-recovery-peer","password":"owned health recovery peer password"}),
    ).await;
    assert_eq!(status, StatusCode::CREATED);
    let peer = account::Account::login(
        &library.app,
        "health-recovery-peer",
        "owned health recovery peer password",
    )
    .await;
    let (status, peer_installation) = peer.request(
        &library.app, "POST", "/api/v1/plugins",
        json!({"service_id":service["id"],"service_revision":service["revision"],"bearer_token":"owned-health-recovery-peer-token"}),
    ).await;
    assert_eq!(status, StatusCode::CREATED);
    assert_ne!(installation["id"], peer_installation["id"]);
    let peer_before = peer.get(&library.app, "/api/v1/plugins").await;
    assert_eq!(peer_before["installations"].as_array().unwrap().len(), 1);
    assert_eq!(
        peer_before["installations"][0]["id"],
        peer_installation["id"]
    );
    assert_eq!(peer_before["installations"][0]["health"]["state"], "ok");
    let before = library.snapshot().await;
    let identities_path = format!("/api/v1/recordings/{}/lyrics/identities", library.recording);
    let identities = library.account.get(&library.app, &identities_path).await;
    assert_eq!(plugin.health_requests().len(), 2);
    plugin.on(HEALTH_PATH, |request| {
        if owner_request(request) {
            Reply::Status(StatusCode::NOT_FOUND)
        } else {
            Reply::Json(serde_json::to_value(HealthReport::ok()).unwrap())
        }
    });
    let cached = library.account.get(&library.app, "/api/v1/plugins").await;
    assert_eq!(
        cached["installations"][0]["health"]["state"],
        "credentials_required"
    );
    assert_eq!(plugin.health_requests().len(), 2);
    let path = format!("/api/v1/recordings/{}/lyrics/candidates", library.recording);
    let request = json!({
        "installation_id":installation["id"],"installation_revision":installation["revision"],
        "query":"Owned health recovery query",
    });
    let (status, unauthenticated) =
        support::call(&library.app, "POST", &path, request.clone(), None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert!(unauthenticated.get("plugin_connection").is_none());
    assert!(plugin.requests_for(SEARCH_PATH).is_empty());
    let (status, headers, response) = account::exchange(
        &library.app,
        "POST",
        &path,
        request,
        library.account.headers(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(headers["cache-control"], "private, no-store");
    assert_eq!(response, json!({"items":[],"next_cursor":null}));
    assert_eq!(plugin.requests_for(SEARCH_PATH).len(), 1);
    let (status, headers, recovered) = account::exchange(
        &library.app,
        "GET",
        "/api/v1/plugins",
        Value::Null,
        library.account.headers(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(headers["cache-control"], "private, no-store");
    let owner_installations = recovered["installations"].as_array().unwrap();
    assert_eq!(owner_installations.len(), 1);
    assert_eq!(owner_installations[0]["id"], installation["id"]);
    assert_eq!(owner_installations[0]["revision"], installation["revision"]);
    assert_eq!(owner_installations[0]["credential_set"], true);
    assert_eq!(recovered["services"], before["plugins"]["services"]);
    let (status, headers, peer_after) = account::exchange(
        &library.app,
        "GET",
        "/api/v1/plugins",
        Value::Null,
        peer.headers(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(headers["cache-control"], "private, no-store");
    assert_eq!(peer_after, peer_before);
    let mut after = library.snapshot().await;
    after["plugins"]["installations"][0]["health"] =
        before["plugins"]["installations"][0]["health"].clone();
    assert_eq!(after, before);
    assert_eq!(
        library.account.get(&library.app, &identities_path).await,
        identities
    );
    library.assert_original();
    assert!((2..=3).contains(&plugin.health_requests().len()));
    assert_eq!(plugin.unexpected(), 0);
    assert!(observed_at.elapsed() < Duration::from_secs(20));
    owner_installations[0].clone()
}
