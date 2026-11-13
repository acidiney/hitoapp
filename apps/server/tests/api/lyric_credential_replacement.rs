use axum::http::{HeaderMap, StatusCode};
use futures_util::FutureExt;
use hito_plugin_sdk::{
    Capability, HEALTH_PATH, MANIFEST_PATH, Manifest, PROTOCOL_VERSION, SEARCH_PATH,
};
use hito_testkit::fake_plugin::{FakePlugin, Gate, RecordedRequest, Reply};
use serde_json::{Value, json};
use std::{
    panic::{AssertUnwindSafe, resume_unwind},
    time::{Duration, Instant},
};
use tokio::task::JoinSet;

use crate::{account, lyric_identity_fixture as library};

const OLD_TOKEN: &str = "owned-replaced-old-service-token";
const NEW_TOKEN: &str = "owned-replaced-new-service-token";
type Response = (StatusCode, HeaderMap, Value);

fn bearer_is(request: &RecordedRequest, token: &str) -> bool {
    request
        .headers
        .get("authorization")
        .is_some_and(|value| value == format!("Bearer {token}").as_str())
}

#[tokio::test]
async fn replacing_credentials_retires_a_pending_old_401_without_stamping_or_poisoning_the_new_revision()
 {
    let mut plugin = FakePlugin::builder(Manifest {
        id: "org.hito.credential-replacement-fixture".into(),
        name: "Owned credential replacement service".into(),
        protocol_version: PROTOCOL_VERSION,
        capabilities: [Capability::Search, Capability::Metadata, Capability::Lyrics].into(),
        authorization: None,
        content_policy: None,
    })
    .require_protocol()
    .get(HEALTH_PATH, |_| Reply::Status(StatusCode::NOT_FOUND))
    .post(SEARCH_PATH, |request| {
        assert!(bearer_is(request, OLD_TOKEN));
        Reply::Status(StatusCode::UNAUTHORIZED)
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
    let (old_status, old_headers, old_response, current_settings) = match result {
        Ok(result) => result,
        Err(panic) => resume_unwind(panic),
    };
    assert_eq!(old_status, StatusCode::CONFLICT);
    assert_eq!(old_headers["cache-control"], "private, no-store");
    assert!(old_response.get("plugin_connection").is_none());
    assert!(old_response.get("code").is_none());
    assert!(
        old_response["error"]
            .as_str()
            .is_some_and(|message| !message.contains(OLD_TOKEN) && !message.contains(NEW_TOKEN))
    );
    assert!(current_settings["installations"][0].get("health").is_none());
}

async fn exercise(
    plugin: &FakePlugin,
    gate: &Gate,
    requests: &mut JoinSet<Response>,
) -> (StatusCode, HeaderMap, Value, Value) {
    let library = library::Library::prepare().await;
    let installation = library
        .account
        .connect(
            &library.app,
            &plugin.url,
            "Owned credential replacement service",
            Some(OLD_TOKEN),
        )
        .await;
    let settings = library.account.get(&library.app, "/api/v1/plugins").await;
    assert!(settings["installations"][0].get("health").is_none());
    let service = settings["services"]
        .as_array()
        .unwrap()
        .iter()
        .find(|service| service["id"] == installation["service_id"])
        .unwrap();
    let (status, _) = library.account.request(
        &library.app, "POST", "/api/v1/users",
        json!({"username":"credential-replacement-peer","password":"owned credential replacement peer password"}),
    ).await;
    assert_eq!(status, StatusCode::CREATED);
    let peer = account::Account::login(
        &library.app,
        "credential-replacement-peer",
        "owned credential replacement peer password",
    )
    .await;
    let (status, peer_installation) = peer.request(
        &library.app, "POST", "/api/v1/plugins",
        json!({"service_id":service["id"],"service_revision":service["revision"],"bearer_token":"owned-replacement-peer-token"}),
    ).await;
    assert_eq!(status, StatusCode::CREATED);
    assert_ne!(installation["id"], peer_installation["id"]);
    let peer_before = peer.get(&library.app, "/api/v1/plugins").await;
    assert_eq!(peer_before["installations"].as_array().unwrap().len(), 1);
    assert_eq!(
        peer_before["installations"][0]["id"],
        peer_installation["id"]
    );
    assert!(peer_before["installations"][0].get("health").is_none());
    let before = library.snapshot().await;
    let identities_path = format!("/api/v1/recordings/{}/lyrics/identities", library.recording);
    let identities = library.account.get(&library.app, &identities_path).await;
    assert_eq!(plugin.health_requests().len(), 2);
    let path = format!("/api/v1/recordings/{}/lyrics/candidates", library.recording);
    let request = json!({
        "installation_id":installation["id"],"installation_revision":installation["revision"],
        "query":"Owned credential replacement query",
    });
    let app = library.app.clone();
    let headers = library.account.headers();
    let started = Instant::now();
    requests.spawn(async move { account::exchange(&app, "POST", &path, request, headers).await });
    assert!(gate.entered_within(Duration::from_secs(1)).await);
    assert_eq!(plugin.requests_for(SEARCH_PATH).len(), 1);
    assert!(requests.try_join_next().is_none());
    assert!(!gate.has_retired());
    let refresh_path = format!(
        "/api/v1/plugins/{}/refresh",
        installation["id"].as_str().unwrap()
    );
    let (status, headers, updated) = account::exchange(
        &library.app, "POST", &refresh_path,
        json!({"revision":installation["revision"],"credential":{"action":"replace","bearer_token":NEW_TOKEN}}),
        library.account.headers(),
    ).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(headers["cache-control"], "private, no-store");
    assert_eq!(updated["kind"], "updated");
    let replacement = &updated["installation"];
    assert_eq!(replacement["id"], installation["id"]);
    assert_eq!(
        replacement["revision"].as_i64().unwrap(),
        installation["revision"].as_i64().unwrap() + 1
    );
    assert_eq!(replacement["service_id"], installation["service_id"]);
    assert_eq!(replacement["manifest"], installation["manifest"]);
    assert_eq!(replacement["credential_set"], true);
    let manifests = plugin.requests_for(MANIFEST_PATH);
    assert_eq!(manifests.len(), 3);
    assert!(bearer_is(manifests.last().unwrap(), NEW_TOKEN));
    let new_settings = library.account.get(&library.app, "/api/v1/plugins").await;
    assert_eq!(new_settings["installations"].as_array().unwrap().len(), 1);
    assert_eq!(new_settings["installations"][0], *replacement);
    assert!(new_settings["installations"][0].get("health").is_none());
    assert_eq!(plugin.health_requests().len(), 3);
    assert!(bearer_is(&plugin.health_requests()[2], NEW_TOKEN));
    let old_result = match requests.try_join_next() {
        Some(result) => {
            gate.open();
            result.unwrap()
        }
        None => {
            gate.release();
            tokio::time::timeout(Duration::from_secs(3), requests.join_next())
                .await
                .unwrap()
                .unwrap()
                .unwrap()
        }
    };
    assert!(gate.retired_within(Duration::from_secs(1)).await);
    let (status, headers, current_settings) = account::exchange(
        &library.app,
        "GET",
        "/api/v1/plugins",
        Value::Null,
        library.account.headers(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(headers["cache-control"], "private, no-store");
    assert_eq!(current_settings, new_settings);
    assert_eq!(current_settings["services"], before["plugins"]["services"]);
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
    assert_eq!(after["plugins"], current_settings);
    after["plugins"] = before["plugins"].clone();
    assert_eq!(after, before);
    assert_eq!(
        library.account.get(&library.app, &identities_path).await,
        identities
    );
    library.assert_original();
    assert_eq!(plugin.requests_for(SEARCH_PATH).len(), 1);
    assert_eq!(plugin.health_requests().len(), 3);
    assert_eq!(plugin.unexpected(), 0);
    assert!(requests.is_empty());
    assert!(started.elapsed() < Duration::from_secs(20));
    (old_result.0, old_result.1, old_result.2, current_settings)
}
