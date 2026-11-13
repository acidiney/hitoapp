use axum::http::StatusCode;
use futures_util::FutureExt;
use hito_core::test_support::hang_guard::HANG_CEILING;
use hito_plugin_sdk::{
    Capability, HEALTH_PATH, HealthReport, MANIFEST_PATH, Manifest, PROTOCOL_VERSION, SEARCH_PATH,
    SearchPage, SearchRequest,
};
use hito_testkit::fake_plugin::{FakePlugin, Reply};
use serde_json::{Value, json};
use std::{
    panic::{AssertUnwindSafe, resume_unwind},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
use uuid::Uuid;

use crate::{account, source_library_fixture as fixture};

const PEER_TOKEN: &str = "owned-source-recovery-peer-bearer";
const OWNER_TOKEN: &str = "owned-source-recovery-original-owner-bearer";

#[tokio::test]
async fn a_new_checked_source_search_clears_only_its_owners_prior_credential_failure() {
    let primary = fixture::ProviderServer::start().await;
    let previous_audio = fixture::ProviderServer::start().await;
    let allow_search = Arc::new(AtomicBool::new(false));
    let search_ready = allow_search.clone();
    let mut plugin = FakePlugin::builder(Manifest {
        id: "org.hito.source-recovery-fixture".into(),
        name: "Owned source recovery service".into(),
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
    .post(SEARCH_PATH, move |request| {
        let search: SearchRequest = serde_json::from_value(request.json()).unwrap();
        assert!(search.validate().is_ok());
        assert!(
            search.query == "Owned performer Owned tune"
                && search.cursor.is_none()
                && search.limit == 20
        );
        let owner = request
            .headers
            .get("authorization")
            .is_some_and(|header| header == format!("Bearer {OWNER_TOKEN}").as_str());
        let peer = request
            .headers
            .get("authorization")
            .is_some_and(|header| header == format!("Bearer {PEER_TOKEN}").as_str());
        assert!(owner || peer);
        if peer || search_ready.load(Ordering::SeqCst) {
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
    let result = tokio::time::timeout(
        HANG_CEILING,
        AssertUnwindSafe(exercise(&primary, &previous_audio, &plugin, &allow_search))
            .catch_unwind(),
    )
    .await;
    let stopped = plugin.finish().await;
    primary.shutdown().await;
    previous_audio.shutdown().await;
    assert!(stopped);
    let installation = match result {
        Ok(Ok(installation)) => installation,
        Ok(Err(panic)) => resume_unwind(panic),
        Err(_) => panic!("the owned source recovery must settle within its deadline"),
    };
    assert_eq!(
        installation["health"]["state"], "ok",
        "A newer valid protected source search must clear only its current owners prior credential diagnosis"
    );
}

async fn exercise(
    primary: &fixture::ProviderServer,
    previous_audio: &fixture::ProviderServer,
    plugin: &FakePlugin,
    allow_search: &AtomicBool,
) -> Value {
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
        .connect(
            &app,
            &plugin.url,
            "Owned source recovery service",
            Some(OWNER_TOKEN),
        )
        .await;
    let (status, owner_headers, owner_before) =
        account::exchange(&app, "GET", "/api/v1/plugins", Value::Null, owner.headers()).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(owner_headers["cache-control"], "private, no-store");
    let current = installation_by_id(&owner_before, &installation["id"]);
    assert_eq!(current["revision"], installation["revision"]);
    assert_eq!(current["service_id"], installation["service_id"]);
    assert_eq!(current["health"]["state"], "ok");
    assert_eq!(current["credential_set"], true);
    let service = owner_before["services"]
        .as_array()
        .unwrap()
        .iter()
        .find(|service| service["id"] == installation["service_id"])
        .unwrap();
    let service_revision = service["revision"].clone();
    let (status, _) = owner.request(&app, "POST", "/api/v1/users",
        json!({"username":"source-recovery-peer","password":"owned source recovery peer password"}),
    ).await;
    assert_eq!(status, StatusCode::CREATED);
    let peer = account::Account::login(
        &app,
        "source-recovery-peer",
        "owned source recovery peer password",
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
    assert!(plugin.requests_for(SEARCH_PATH).is_empty());
    let started = Instant::now();
    let (status, headers, resolution) =
        account::exchange(&app, "POST", &path, request.clone(), owner.headers()).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(headers["cache-control"], "private, no-store");
    assert_eq!(resolution["request_id"], request_id.to_string());
    assert_eq!(resolution["recording_id"], owned.recording);
    assert_eq!(resolution["outcome"], "provider_unavailable");
    assert!(resolution["sources"].as_array().unwrap().is_empty());
    assert!(resolution.get("plugin_connection").is_none());
    let calls = plugin.requests_for(SEARCH_PATH);
    assert_eq!(calls.len(), 1);
    assert!(
        calls[0]
            .headers
            .get("authorization")
            .is_some_and(|header| header == format!("Bearer {OWNER_TOKEN}").as_str())
    );
    let (status, headers, replay) =
        account::exchange(&app, "POST", &path, request.clone(), owner.headers()).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(headers["cache-control"], "private, no-store");
    assert_eq!(replay, resolution);
    let (status, _) = peer.request(&app, "POST", &path, request.clone()).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let diagnosed = owner.get(&app, "/api/v1/plugins").await;
    assert_eq!(
        installation_by_id(&diagnosed, &installation["id"])["health"]["state"],
        "credentials_required"
    );
    assert_eq!(
        installation_by_id(&diagnosed, &installation["id"])["revision"],
        installation["revision"]
    );
    assert_eq!(
        diagnosed["services"]
            .as_array()
            .unwrap()
            .iter()
            .find(|service| service["id"] == installation["service_id"])
            .unwrap()["revision"],
        service_revision
    );
    assert_eq!(owner.get(&app, "/api/v1/plugins").await, diagnosed);
    let (status, declaration_headers, declaration) = account::exchange(
        &app,
        "POST",
        &format!(
            "/api/v1/plugins/{}/declaration-check",
            installation["id"].as_str().unwrap()
        ),
        json!({"revision":installation["revision"]}),
        owner.headers(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(declaration_headers["cache-control"], "private, no-store");
    assert_eq!(declaration["kind"], "current");
    let after_public = owner.get(&app, "/api/v1/plugins").await;
    assert_eq!(after_public, diagnosed);
    assert_eq!(plugin.requests_for(MANIFEST_PATH).len(), 3);
    assert_eq!(plugin.health_requests().len(), 2);
    assert_eq!(plugin.requests_for(SEARCH_PATH).len(), 1);
    allow_search.store(true, Ordering::SeqCst);
    let (status, old_replay) = owner.request(&app, "POST", &path, request.clone()).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(old_replay, resolution);
    assert_eq!(owner.get(&app, "/api/v1/plugins").await, diagnosed);
    assert_eq!(plugin.requests_for(SEARCH_PATH).len(), 1);
    let retry_id = Uuid::new_v4();
    assert_ne!(retry_id, request_id);
    let retry_request = json!({"request_id":retry_id});
    let (status, retry_headers, retry) =
        account::exchange(&app, "POST", &path, retry_request.clone(), owner.headers()).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(retry_headers["cache-control"], "private, no-store");
    assert_eq!(retry["request_id"], retry_id.to_string());
    assert_eq!(retry["recording_id"], owned.recording);
    assert_ne!(retry["resolution_id"], resolution["resolution_id"]);
    assert_eq!(retry["outcome"], "no_suitable_source");
    assert!(retry["sources"].as_array().unwrap().is_empty());
    assert!(retry.get("plugin_connection").is_none());
    let searches = plugin.requests_for(SEARCH_PATH);
    assert_eq!(searches.len(), 2);
    assert!(searches.iter().all(|call| {
        call.headers
            .get("authorization")
            .is_some_and(|header| header == format!("Bearer {OWNER_TOKEN}").as_str())
    }));
    let (status, retry_replay) = owner
        .request(&app, "POST", &path, retry_request.clone())
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(retry_replay, retry);
    let (status, first_replay) = owner.request(&app, "POST", &path, request.clone()).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(first_replay, resolution);
    let (status, _) = peer.request(&app, "POST", &path, retry_request).await;
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
    assert_eq!(plugin.requests_for(MANIFEST_PATH).len(), 3);
    assert_eq!(plugin.requests_for(SEARCH_PATH).len(), 2);
    assert_eq!(plugin.requests().len(), 5);
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
        choice
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
    let receipt = serde_json::to_string(&json!([resolution, retry])).unwrap();
    for private in [
        OWNER_TOKEN,
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
    current.clone()
}

fn installation_by_id<'a>(settings: &'a Value, id: &Value) -> &'a Value {
    settings["installations"]
        .as_array()
        .unwrap()
        .iter()
        .find(|installation| &installation["id"] == id)
        .unwrap()
}
