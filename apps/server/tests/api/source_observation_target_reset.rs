use axum::http::{HeaderMap, StatusCode};
use futures_util::FutureExt;
use hito_core::test_support::hang_guard::HANG_CEILING;
use hito_plugin_sdk::{
    Capability, HEALTH_PATH, HealthReport, MANIFEST_PATH, Manifest, PROTOCOL_VERSION, SEARCH_PATH,
    SearchPage, SearchRequest,
};
use hito_testkit::fake_plugin::{FakePlugin, Gate, Reply};
use serde_json::{Value, json};
use std::{
    panic::{AssertUnwindSafe, resume_unwind},
    time::{Duration, Instant},
};
use tokio::task::JoinSet;
use uuid::Uuid;

type Response = (StatusCode, HeaderMap, Value);

use crate::{account, source_library_fixture as fixture};

const PEER_TOKEN: &str = "owned-source-target-reset-peer-bearer";

#[tokio::test]
async fn resetting_the_target_while_search401_is_held_cancels_without_observing_credentials() {
    let primary = fixture::ProviderServer::start().await;
    let previous_audio = fixture::ProviderServer::start().await;
    let mut plugin = FakePlugin::builder(Manifest {
        id: "org.hito.source-target-reset-fixture".into(),
        name: "Owned source target reset service".into(),
        protocol_version: PROTOCOL_VERSION,
        capabilities: [
            Capability::Search,
            Capability::Sources,
            Capability::Metadata,
        ]
        .into(),
        authorization: None,
        content_policy: None,
    })
    .require_protocol()
    .get(HEALTH_PATH, |_| {
        Reply::Json(serde_json::to_value(HealthReport::ok()).unwrap())
    })
    .post(SEARCH_PATH, |request| {
        let search: SearchRequest = serde_json::from_value(request.json()).unwrap();
        assert!(search.validate().is_ok());
        assert!(
            search.query == "Owned performer Owned tune"
                && search.cursor.is_none()
                && search.limit == 20
        );
        if request.has_authorization() {
            assert!(
                request
                    .headers
                    .get("authorization")
                    .is_some_and(|header| header == format!("Bearer {PEER_TOKEN}").as_str())
            );
            let page = SearchPage {
                items: Vec::new(),
                next_cursor: None,
            };
            assert!(page.validate(20).is_ok());
            Reply::Json(serde_json::to_value(page).unwrap())
        } else {
            Reply::Status(StatusCode::UNAUTHORIZED)
        }
    })
    .start()
    .await;
    plugin.set_hold_limit(Duration::from_secs(4));
    let gate = plugin.hold_next(SEARCH_PATH);
    let mut requests = JoinSet::new();
    let result = tokio::time::timeout(
        HANG_CEILING,
        AssertUnwindSafe(exercise(
            &primary,
            &previous_audio,
            &plugin,
            &gate,
            &mut requests,
        ))
        .catch_unwind(),
    )
    .await;
    gate.open();
    requests.abort_all();
    tokio::time::timeout(Duration::from_secs(3), async {
        while requests.join_next().await.is_some() {}
    })
    .await
    .unwrap();
    let stopped = plugin.finish().await;
    primary.shutdown().await;
    previous_audio.shutdown().await;
    assert!(stopped);
    let (resolution, installation) = match result {
        Ok(Ok(result)) => result,
        Ok(Err(panic)) => resume_unwind(panic),
        Err(_) => panic!("the owned target reset must settle within its deadline"),
    };
    assert_eq!(
        resolution["outcome"], "cancelled",
        "A target reset must retain the existing cancelled completion for the old source request"
    );
    assert_eq!(
        installation["health"]["state"], "ok",
        "A rejected response for the old target must not publish a current credential observation"
    );
}

async fn exercise(
    primary: &fixture::ProviderServer,
    previous_audio: &fixture::ProviderServer,
    plugin: &FakePlugin,
    gate: &Gate,
    requests: &mut JoinSet<Response>,
) -> (Value, Value) {
    let owned = fixture::SavedLibrary::prepare(primary, previous_audio).await;
    let previous = owned
        .hito
        .plugins
        .connections(&owned.actor)
        .await
        .unwrap()
        .installations
        .into_iter()
        .find(|installation| installation.manifest.id == "org.hito.audio-fixture")
        .unwrap();
    owned
        .hito
        .plugins
        .remove(&owned.actor, &previous.id, previous.revision)
        .await
        .unwrap();
    let app = hito_server::router(owned.hito.clone());
    let owner = account::Account::login(&app, "admin", "indexing test password").await;
    let installation = owner
        .connect(&app, &plugin.url, "Owned source target reset service", None)
        .await;
    let (status, owner_headers, owner_before) =
        account::exchange(&app, "GET", "/api/v1/plugins", Value::Null, owner.headers()).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(owner_headers["cache-control"], "private, no-store");
    let current = installation_by_id(&owner_before, &installation["id"]);
    assert_eq!(current["revision"], installation["revision"]);
    assert_eq!(current["service_id"], installation["service_id"]);
    assert_eq!(current["health"]["state"], "ok");
    assert_eq!(current["credential_set"], false);
    let service = owner_before["services"]
        .as_array()
        .unwrap()
        .iter()
        .find(|service| service["id"] == installation["service_id"])
        .unwrap();
    let service_revision = service["revision"].clone();
    let (status, _) = owner.request(&app, "POST", "/api/v1/users",
        json!({"username":"source-target-reset-peer","password":"owned source target reset peer password"}),
    ).await;
    assert_eq!(status, StatusCode::CREATED);
    let peer = account::Account::login(
        &app,
        "source-target-reset-peer",
        "owned source target reset peer password",
    )
    .await;
    let (status, peer_installation) = peer.request(&app, "POST", "/api/v1/plugins", json!({
        "service_id":service["id"],"service_revision":service_revision,"bearer_token":PEER_TOKEN,
    })).await;
    assert_eq!(status, StatusCode::CREATED);
    assert_ne!(peer_installation["id"], installation["id"]);
    let (status, peer_headers, peer_before) =
        account::exchange(&app, "GET", "/api/v1/plugins", Value::Null, peer.headers()).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(peer_headers["cache-control"], "private, no-store");
    assert_eq!(peer_before["installations"].as_array().unwrap().len(), 1);
    let peer_current = installation_by_id(&peer_before, &peer_installation["id"]);
    assert_eq!(peer_current["health"]["state"], "ok");
    assert_eq!(peer_current["credential_set"], true);
    assert_eq!(plugin.health_requests().len(), 2);
    assert!(plugin.requests_for(SEARCH_PATH).is_empty());
    let path = format!("/api/v1/recordings/{}/sources/resolve", owned.recording);
    let request_id = Uuid::new_v4();
    let request = json!({"request_id":request_id});
    let (status, unauthenticated) =
        crate::support::call(&app, "POST", &path, request.clone(), None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert!(unauthenticated.get("plugin_connection").is_none());
    let (status, peer_error) = peer.request(&app, "POST", &path, request.clone()).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(peer_error.get("plugin_connection").is_none());
    let choice_path = format!("/api/v1/recordings/{}/source-choice", owned.recording);
    let (status, _) = peer.request(&app, "GET", &choice_path, Value::Null).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let choice = serde_json::to_value(
        owned
            .hito
            .catalog
            .source_choice(&owned.actor, &owned.recording)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(choice["mode"], "automatic");
    assert_eq!(choice["revision"], 0);
    assert!(plugin.requests_for(SEARCH_PATH).is_empty());
    let started = Instant::now();
    let pending_app = app.clone();
    let pending_headers = owner.headers();
    let pending_path = path.clone();
    let pending_request = request.clone();
    requests.spawn(async move {
        account::exchange(
            &pending_app,
            "POST",
            &pending_path,
            pending_request,
            pending_headers,
        )
        .await
    });
    assert!(gate.entered_within(Duration::from_secs(1)).await);
    assert_eq!(plugin.requests_for(SEARCH_PATH).len(), 1);
    assert!(!gate.has_retired());
    assert!(requests.try_join_next().is_none());
    let reset_id = Uuid::new_v4();
    let (status, reset_headers, reset) = account::exchange(
        &app,
        "POST",
        &format!("{choice_path}/reset"),
        json!({"request_id":reset_id,"expected_revision":choice["revision"]}),
        owner.headers(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(reset_headers["cache-control"], "private, no-store");
    assert_eq!(reset["request_id"], reset_id.to_string());
    assert_eq!(reset["recording_id"], owned.recording);
    assert_eq!(reset["revision"], 1);
    let reset_choice = owner.get(&app, &choice_path).await;
    assert_eq!(reset_choice["mode"], "automatic");
    assert_eq!(reset_choice["revision"], 1);
    assert!(reset_choice["sources"].as_array().unwrap().is_empty());
    assert!(!gate.has_retired());
    assert!(requests.try_join_next().is_none());
    gate.release();
    let (status, headers, resolution) = requests.join_next().await.unwrap().unwrap();
    assert!(gate.has_retired());
    assert_eq!(status, StatusCode::OK);
    assert_eq!(headers["cache-control"], "private, no-store");
    assert_eq!(resolution["request_id"], request_id.to_string());
    assert_eq!(resolution["recording_id"], owned.recording);
    assert!(resolution["sources"].as_array().unwrap().is_empty());
    assert!(resolution.get("plugin_connection").is_none());
    let calls = plugin.requests_for(SEARCH_PATH);
    assert_eq!(calls.len(), 1);
    assert!(!calls[0].has_authorization());
    let (status, headers, replay) =
        account::exchange(&app, "POST", &path, request.clone(), owner.headers()).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(headers["cache-control"], "private, no-store");
    assert_eq!(replay, resolution);
    let (status, _) = peer.request(&app, "POST", &path, request).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, headers, owner_after) =
        account::exchange(&app, "GET", "/api/v1/plugins", Value::Null, owner.headers()).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(headers["cache-control"], "private, no-store");
    let (status, headers, peer_after) =
        account::exchange(&app, "GET", "/api/v1/plugins", Value::Null, peer.headers()).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(headers["cache-control"], "private, no-store");
    assert_eq!(peer_after, peer_before);
    let current = installation_by_id(&owner_after, &installation["id"]);
    assert_eq!(current["revision"], installation["revision"]);
    assert_eq!(current["service_id"], installation["service_id"]);
    let current_service = owner_after["services"]
        .as_array()
        .unwrap()
        .iter()
        .find(|service| service["id"] == installation["service_id"])
        .unwrap();
    assert_eq!(current_service["revision"], service_revision);
    let mut before = owner_before;
    let mut after = owner_after.clone();
    for value in [&mut before, &mut after] {
        let installation = value["installations"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|value| value["id"] == current["id"])
            .unwrap();
        installation.as_object_mut().unwrap().remove("health");
    }
    assert_eq!(after, before);
    assert_eq!(plugin.health_requests().len(), 2);
    assert_eq!(plugin.requests_for(MANIFEST_PATH).len(), 2);
    assert_eq!(plugin.requests_for(SEARCH_PATH).len(), 1);
    assert_eq!(plugin.requests().len(), 3);
    assert_eq!(plugin.unexpected(), 0);
    owned.assert_preserved().await;
    assert_eq!(
        serde_json::to_value(
            owned
                .hito
                .catalog
                .source_choice(&owned.actor, &owned.recording)
                .await
                .unwrap()
        )
        .unwrap(),
        reset_choice
    );
    for path in [
        "v1/search",
        "v1/recordings/detail",
        "v1/recordings/metadata",
        "v1/sources",
        "v1/audio",
    ] {
        assert!(previous_audio.requests(path).await.is_empty());
    }
    let receipt = serde_json::to_string(&resolution).unwrap();
    for private in [
        PEER_TOKEN,
        "Private title override",
        "primary/owned-recording",
        primary.url.as_str(),
        previous_audio.url.as_str(),
        plugin.url.as_str(),
    ] {
        assert!(!receipt.contains(private));
    }
    assert!(started.elapsed() < Duration::from_secs(20));
    (resolution, current.clone())
}

fn installation_by_id<'a>(settings: &'a Value, id: &Value) -> &'a Value {
    settings["installations"]
        .as_array()
        .unwrap()
        .iter()
        .find(|installation| &installation["id"] == id)
        .unwrap()
}
