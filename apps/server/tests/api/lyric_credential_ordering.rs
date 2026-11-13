use axum::http::{HeaderMap, StatusCode};
use futures_util::FutureExt;
use hito_plugin_sdk::{
    Capability, HEALTH_PATH, HealthReport, Manifest, PROTOCOL_VERSION, SEARCH_PATH, SearchPage,
};
use hito_testkit::fake_plugin::{FakePlugin, Gate, Reply};
use serde_json::{Value, json};
use std::{
    panic::{AssertUnwindSafe, resume_unwind},
    time::{Duration, Instant},
};
use tokio::task::JoinSet;

use crate::{account, lyric_identity_fixture as library};

const TOKEN: &str = "owned-credential-ordering-token";
type Response = (StatusCode, HeaderMap, Value);

fn owner_request(request: &hito_testkit::fake_plugin::RecordedRequest) -> bool {
    request
        .headers
        .get("authorization")
        .is_some_and(|value| value == format!("Bearer {TOKEN}").as_str())
}

#[tokio::test]
async fn an_older_held_search_success_cannot_clear_a_newer_health_credential_failure() {
    let mut plugin = FakePlugin::builder(Manifest {
        id: "org.hito.credential-ordering-fixture".into(),
        name: "Owned credential ordering service".into(),
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
    plugin.set_hold_limit(Duration::from_secs(4));
    let gate = plugin.hold_next(SEARCH_PATH);
    let mut requests = JoinSet::new();
    let result = AssertUnwindSafe(exercise(&plugin, &gate, &mut requests))
        .catch_unwind()
        .await;
    gate.open();
    requests.abort_all();
    tokio::time::timeout(Duration::from_secs(3), async {
        while requests.join_next().await.is_some() {}
    })
    .await
    .unwrap();
    assert!(plugin.finish().await);
    let installation = match result {
        Ok(installation) => installation,
        Err(panic) => resume_unwind(panic),
    };
    assert_eq!(
        installation["health"]["state"], "credentials_required",
        "An older protected success must preserve the newer health credential failure"
    );
}

async fn exercise(plugin: &FakePlugin, gate: &Gate, requests: &mut JoinSet<Response>) -> Value {
    let library = library::Library::prepare().await;
    let (status, service) = library
        .account
        .request(
            &library.app,
            "POST",
            "/api/v1/plugins/services",
            json!({"name":"Owned credential ordering service","base_url":plugin.url}),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED);
    let (status, installation) = library.account.request(
        &library.app, "POST", "/api/v1/plugins",
        json!({"service_id":service["id"],"service_revision":service["revision"],"bearer_token":TOKEN}),
    ).await;
    assert_eq!(status, StatusCode::CREATED);
    let (status, _) = library.account.request(
        &library.app, "POST", "/api/v1/users",
        json!({"username":"credential-ordering-peer","password":"owned credential ordering peer password"}),
    ).await;
    assert_eq!(status, StatusCode::CREATED);
    let peer = account::Account::login(
        &library.app,
        "credential-ordering-peer",
        "owned credential ordering peer password",
    )
    .await;
    let (status, peer_installation) = peer.request(
        &library.app, "POST", "/api/v1/plugins",
        json!({"service_id":service["id"],"service_revision":service["revision"],"bearer_token":"owned-ordering-peer-token"}),
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
    assert_eq!(plugin.health_requests().len(), 1);
    assert!(
        plugin
            .health_requests()
            .iter()
            .all(|request| !owner_request(request))
    );
    let path = format!("/api/v1/recordings/{}/lyrics/candidates", library.recording);
    let request = json!({
        "installation_id":installation["id"],"installation_revision":installation["revision"],
        "query":"Owned credential ordering query",
    });
    let app = library.app.clone();
    let headers = library.account.headers();
    requests.spawn(async move { account::exchange(&app, "POST", &path, request, headers).await });
    assert!(gate.entered_within(Duration::from_secs(1)).await);
    assert_eq!(plugin.requests_for(SEARCH_PATH).len(), 1);
    assert_eq!(requests.len(), 1);
    assert!(requests.try_join_next().is_none());
    let held_at = Instant::now();
    let (status, headers, diagnosed) = account::exchange(
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
        diagnosed["installations"][0]["health"]["state"],
        "credentials_required"
    );
    assert_eq!(plugin.health_requests().len(), 2);
    assert!(owner_request(&plugin.health_requests()[1]));
    let observed_at = Instant::now();
    let before = library.snapshot().await;
    let identities_path = format!("/api/v1/recordings/{}/lyrics/identities", library.recording);
    let identities = library.account.get(&library.app, &identities_path).await;
    assert!(requests.try_join_next().is_none());
    assert!(held_at.elapsed() < Duration::from_secs(4));
    assert!(!gate.has_retired());
    gate.release();
    let (status, headers, response) =
        tokio::time::timeout(Duration::from_secs(3), requests.join_next())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
    assert_eq!(status, StatusCode::OK);
    assert_eq!(headers["cache-control"], "private, no-store");
    assert_eq!(response, json!({"items":[],"next_cursor":null}));
    assert!(gate.retired_within(Duration::from_secs(1)).await);
    let (status, headers, after_settings) = account::exchange(
        &library.app,
        "GET",
        "/api/v1/plugins",
        Value::Null,
        library.account.headers(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(headers["cache-control"], "private, no-store");
    let owner_installations = after_settings["installations"].as_array().unwrap();
    assert_eq!(owner_installations.len(), 1);
    assert_eq!(owner_installations[0]["id"], installation["id"]);
    assert_eq!(owner_installations[0]["revision"], installation["revision"]);
    assert_eq!(owner_installations[0]["credential_set"], true);
    assert_eq!(after_settings["services"], before["plugins"]["services"]);
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
    assert_eq!(library.snapshot().await, before);
    assert_eq!(
        library.account.get(&library.app, &identities_path).await,
        identities
    );
    library.assert_original();
    assert_eq!(plugin.requests_for(SEARCH_PATH).len(), 1);
    assert_eq!(plugin.health_requests().len(), 2);
    assert_eq!(plugin.unexpected(), 0);
    assert!(requests.is_empty());
    assert!(observed_at.elapsed() < Duration::from_secs(20));
    owner_installations[0].clone()
}

#[tokio::test]
async fn a_newer_held_search_success_clears_a_rejection_from_an_earlier_dispatch() {
    let mut plugin = FakePlugin::builder(Manifest {
        id: "org.hito.overlapping-credential-ordering".into(),
        name: "Owned overlapping credential service".into(),
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
        assert!(request.query.is_none());
        let search: hito_plugin_sdk::SearchRequest =
            serde_json::from_value(request.json()).unwrap();
        assert!(search.validate().is_ok());
        assert_eq!(search.limit, 20);
        assert!(search.cursor.is_none());
        assert_eq!(search.query, "Owned later successful query");
        Reply::Json(json!({"items":[],"next_cursor":null}))
    })
    .start()
    .await;
    plugin.set_hold_limit(Duration::from_secs(4));
    let mut gates = Vec::new();
    let mut requests = JoinSet::new();
    let result = AssertUnwindSafe(Box::pin(overlapping_dispatches(
        &plugin,
        &mut gates,
        &mut requests,
    )))
    .catch_unwind()
    .await;
    for gate in gates {
        gate.open();
    }
    requests.abort_all();
    tokio::time::timeout(Duration::from_secs(3), async {
        while requests.join_next().await.is_some() {}
    })
    .await
    .unwrap();
    assert!(plugin.finish().await);
    let installation = match result {
        Ok(installation) => installation,
        Err(panic) => resume_unwind(panic),
    };
    assert_eq!(
        installation.get("health"),
        None,
        "A newer checked success must clear an earlier-dispatched rejection even when that rejection completed after the newer dispatch"
    );
}

async fn overlapping_dispatches(
    plugin: &FakePlugin,
    gates: &mut Vec<std::sync::Arc<Gate>>,
    requests: &mut JoinSet<Response>,
) -> Value {
    let library = library::Library::prepare().await;
    let (status, service) = library
        .account
        .request(
            &library.app,
            "POST",
            "/api/v1/plugins/services",
            json!({"name":"Owned overlapping credential service","base_url":plugin.url}),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED);
    let (status, installation) = library.account.request(&library.app, "POST", "/api/v1/plugins",
        json!({"service_id":service["id"],"service_revision":service["revision"],"bearer_token":TOKEN})).await;
    assert_eq!(status, StatusCode::CREATED);
    let (status, _) = library
        .account
        .request(
            &library.app,
            "POST",
            "/api/v1/users",
            json!({"username":"overlapping-peer","password":"owned overlapping peer password"}),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED);
    let peer = account::Account::login(
        &library.app,
        "overlapping-peer",
        "owned overlapping peer password",
    )
    .await;
    let (status, peer_installation) = peer.request(&library.app, "POST", "/api/v1/plugins",
        json!({"service_id":service["id"],"service_revision":service["revision"],"bearer_token":"owned-overlapping-peer-token"})).await;
    assert_eq!(status, StatusCode::CREATED);
    assert_ne!(peer_installation["id"], installation["id"]);
    let peer_before = peer.get(&library.app, "/api/v1/plugins").await;
    assert_eq!(peer_before["installations"][0]["health"]["state"], "ok");
    let playlist = uuid::Uuid::new_v4();
    let operations = json!([
        {"id":uuid::Uuid::new_v4(),"action":{"kind":"like_set","recording_id":library.recording,"liked":true}},
        {"id":uuid::Uuid::new_v4(),"action":{"kind":"playlist_create","playlist_id":playlist,"name":"Owned overlap private playlist"}},
        {"id":uuid::Uuid::new_v4(),"action":{"kind":"entry_insert","playlist_id":playlist,"entry_id":uuid::Uuid::new_v4(),"recording_id":library.recording,"position":0}}
    ]);
    let (status, receipt) = library
        .account
        .request(
            &library.app,
            "POST",
            "/api/v1/sync",
            json!({"device_id":"owned-overlap-client","operations":operations}),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(receipt["acknowledged"].as_array().unwrap().len(), 3);
    let before = overlap_snapshot(&library).await;
    let base = format!("/api/v1/recordings/{}/lyrics/identities", library.recording);
    let identities = library.account.get(&library.app, &base).await;
    let (peer_identity_status, peer_identity_headers, peer_identities) =
        account::exchange(&library.app, "GET", &base, Value::Null, peer.headers()).await;
    assert_eq!(peer_identity_status, StatusCode::NOT_FOUND);
    assert_eq!(peer_identity_headers["cache-control"], "private, no-store");
    assert!(peer_identities.get("plugin_connection").is_none());
    let playlists = library.account.get(&library.app, "/api/v1/playlists").await;
    let peer_playlists = peer.get(&library.app, "/api/v1/playlists").await;
    assert_eq!(plugin.health_requests().len(), 1);
    assert!(!owner_request(&plugin.health_requests()[0]));
    assert!(plugin.requests_for(SEARCH_PATH).is_empty());
    gates.push(plugin.hold_next(HEALTH_PATH));
    let app = library.app.clone();
    let headers = library.account.headers();
    requests.spawn(async move {
        account::exchange(&app, "GET", "/api/v1/plugins", Value::Null, headers).await
    });
    assert!(gates[0].entered_within(Duration::from_secs(1)).await);
    assert_eq!(plugin.health_requests().len(), 2);
    assert!(owner_request(&plugin.health_requests()[1]));
    let held_at = Instant::now();
    gates.push(plugin.hold_next(SEARCH_PATH));
    let path = format!("/api/v1/recordings/{}/lyrics/candidates", library.recording);
    let request = json!({"installation_id":installation["id"],"installation_revision":installation["revision"],"query":"Owned later successful query"});
    let app = library.app.clone();
    let headers = library.account.headers();
    requests.spawn(async move { account::exchange(&app, "POST", &path, request, headers).await });
    assert!(gates[1].entered_within(Duration::from_secs(1)).await);
    assert_eq!(plugin.requests_for(SEARCH_PATH).len(), 1);
    assert!(requests.try_join_next().is_none());
    gates[0].release();
    let (status, headers, rejected) =
        tokio::time::timeout(Duration::from_secs(3), requests.join_next())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
    assert_eq!(status, StatusCode::OK);
    assert_eq!(headers["cache-control"], "private, no-store");
    assert_eq!(
        rejected["installations"][0]["health"]["state"],
        "credentials_required"
    );
    assert_eq!(rejected["installations"][0]["id"], installation["id"]);
    assert_eq!(
        rejected["installations"][0]["revision"],
        installation["revision"]
    );
    assert!(rejected.get("plugin_connection").is_none());
    assert!(held_at.elapsed() < Duration::from_secs(2));
    assert!(gates[0].retired_within(Duration::from_secs(1)).await);
    assert!(!gates[1].has_retired());
    assert!(requests.try_join_next().is_none());
    let (status, headers, diagnosed) = account::exchange(
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
        diagnosed["installations"][0]["health"]["state"],
        "credentials_required"
    );
    gates[1].release();
    let (status, headers, recovered) =
        tokio::time::timeout(Duration::from_secs(3), requests.join_next())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
    assert_eq!(status, StatusCode::OK);
    assert_eq!(headers["cache-control"], "private, no-store");
    assert_eq!(recovered, json!({"items":[],"next_cursor":null}));
    assert!(gates[1].retired_within(Duration::from_secs(1)).await);
    assert!(requests.is_empty());
    let (status, headers, owner_after) = account::exchange(
        &library.app,
        "GET",
        "/api/v1/plugins",
        Value::Null,
        library.account.headers(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(headers["cache-control"], "private, no-store");
    let current = owner_after["installations"][0].clone();
    assert_eq!(overlap_snapshot(&library).await, before);
    assert_eq!(owner_after["installations"].as_array().unwrap().len(), 1);
    let mut connection = current.clone();
    connection.as_object_mut().unwrap().remove("health");
    assert_eq!(connection, installation);
    assert_eq!(current["credential_set"], true);
    assert_eq!(owner_after["services"], json!([service]));
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
    assert_eq!(library.account.get(&library.app, &base).await, identities);
    let (peer_after_status, peer_after_headers, peer_after_identities) =
        account::exchange(&library.app, "GET", &base, Value::Null, peer.headers()).await;
    assert_eq!(peer_after_status, StatusCode::NOT_FOUND);
    assert_eq!(peer_after_headers["cache-control"], "private, no-store");
    assert!(peer_after_identities.get("plugin_connection").is_none());
    assert_eq!(
        (peer_after_status, peer_after_identities),
        (peer_identity_status, peer_identities)
    );
    assert_eq!(
        library.account.get(&library.app, "/api/v1/playlists").await,
        playlists
    );
    assert_eq!(
        peer.get(&library.app, "/api/v1/playlists").await,
        peer_playlists
    );
    library.assert_original();
    assert_eq!(plugin.health_requests().len(), 2);
    assert_eq!(plugin.requests_for(SEARCH_PATH).len(), 1);
    assert_eq!(
        plugin
            .health_requests()
            .iter()
            .filter(|request| owner_request(request))
            .count(),
        1
    );
    assert_eq!(
        plugin
            .health_requests()
            .iter()
            .filter(|request| !owner_request(request))
            .count(),
        1
    );
    assert!(held_at.elapsed() < Duration::from_secs(4));
    assert_eq!(
        plugin
            .requests()
            .iter()
            .filter(|request| !matches!(request.operation.as_str(), "v1/manifest" | "v1/search"))
            .count(),
        0
    );
    assert_eq!(plugin.unexpected(), 0);
    current
}

async fn overlap_snapshot(library: &library::Library) -> Value {
    let base = format!("/api/v1/recordings/{}", library.recording);
    json!({
        "catalog":library.account.get(&library.app, "/api/v1/recordings").await,
        "metadata":library.account.get(&library.app, &format!("{base}/metadata")).await,
        "sources":library.account.get(&library.app, &format!("{base}/sources")).await,
        "libraries":library.account.get(&library.app, "/api/v1/libraries").await,
        "artists":library.account.get(&library.app, "/api/v1/artists").await,
        "releases":library.account.get(&library.app, "/api/v1/releases").await,
        "activity":library.account.get(&library.app, "/api/v1/activity").await,
        "events":library.account.get(&library.app, "/api/v1/sync?cursor=0").await,
    })
}
