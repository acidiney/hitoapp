use axum::http::StatusCode;
use futures_util::{FutureExt, future::join_all};
use hito_plugin_sdk::{
    Capability, HEALTH_PATH, HealthReport, MANIFEST_PATH, Manifest, PROTOCOL_VERSION, SEARCH_PATH,
    SearchPage, SearchRequest,
};
use hito_testkit::fake_plugin::{FakePlugin, Gate, Reply};
use serde_json::{Value, json};
use std::{
    collections::HashSet,
    panic::{AssertUnwindSafe, resume_unwind},
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::task::JoinSet;
use uuid::Uuid;

use crate::credential_pressure_fixture as fixture;
use fixture::{Context, OWNER_TOKEN, Response, account, installation_by_id};

#[tokio::test]
async fn other_accounts_current_generation_pressure_cannot_revive_an_older_source_credential_failure()
 {
    let primary = fixture::ProviderServer::start().await;
    let previous = fixture::ProviderServer::start().await;
    let mut main = FakePlugin::builder(Manifest {
        id: "org.hito.source-credential-pressure".into(),
        name: "Owned source pressure service".into(),
        protocol_version: PROTOCOL_VERSION,
        capabilities: [
            Capability::Search,
            Capability::Sources,
            Capability::Metadata,
            Capability::Lyrics,
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
        assert!(
            request
                .headers
                .get("authorization")
                .is_some_and(|header| header == format!("Bearer {OWNER_TOKEN}").as_str())
        );
        let search: SearchRequest = serde_json::from_value(request.json()).unwrap();
        assert!(search.validate().is_ok());
        assert!(
            search.query == "Owned performer Owned tune"
                && search.cursor.is_none()
                && search.limit == 20
        );
        Reply::Status(StatusCode::UNAUTHORIZED)
    })
    .start()
    .await;
    main.set_hold_limit(Duration::from_secs(4));
    let mut others = Vec::new();
    let mut requests = JoinSet::new();
    let mut held = None;
    let result = tokio::time::timeout(
        Duration::from_secs(60),
        AssertUnwindSafe(Box::pin(exercise(
            &primary,
            &previous,
            &main,
            &mut others,
            &mut requests,
            &mut held,
        )))
        .catch_unwind(),
    )
    .await;
    if let Some(gate) = held {
        gate.open();
    }
    requests.abort_all();
    tokio::time::timeout(Duration::from_secs(3), async {
        while requests.join_next().await.is_some() {}
    })
    .await
    .unwrap();
    let mut stopped = main.finish().await;
    for mut plugin in others {
        stopped &= plugin.finish().await;
    }
    primary.shutdown().await;
    previous.shutdown().await;
    assert!(stopped);
    let (resolution, owner) = match result {
        Ok(Ok(result)) => result,
        Ok(Err(panic)) => resume_unwind(panic),
        Err(_) => panic!("the owned pressure fixture must settle within its outer deadline"),
    };
    assert_eq!(resolution["outcome"], "provider_unavailable");
    assert_eq!(
        owner["health"]["state"], "ok",
        "Current-generation traffic from other accounts must not let an older source401 replace a newer protected success"
    );
}

async fn exercise(
    primary: &fixture::ProviderServer,
    previous: &fixture::ProviderServer,
    main: &FakePlugin,
    others: &mut Vec<FakePlugin>,
    requests: &mut JoinSet<Response>,
    held: &mut Option<Arc<Gate>>,
) -> (Value, Value) {
    let context = Context::prepare(primary, previous, main).await;
    let peers = fixture::prepare(&context, others).await;
    let owner_before = context.settings().await;
    assert_eq!(
        installation_by_id(&owner_before, &context.installation["id"])["health"]["state"],
        "ok"
    );
    let choice = context.choice.clone();
    let identities_path = format!(
        "/api/v1/recordings/{}/lyrics/identities",
        context.owned.recording
    );
    let identities = context.owner.get(&context.app, &identities_path).await;
    let main_health_before = main.health_requests().len();
    let other_health_before: Vec<_> = others
        .iter()
        .map(|plugin| plugin.health_requests().len())
        .collect();
    let main_calls_before = main.requests().len();
    assert_eq!(main.requests_for(MANIFEST_PATH).len(), 5);
    assert!(main.requests_for(SEARCH_PATH).is_empty());
    for peer in &peers {
        let (status, error) = peer
            .account
            .request(
                &context.app,
                "POST",
                &context.path(),
                json!({"request_id":Uuid::new_v4()}),
            )
            .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert!(error.get("plugin_connection").is_none());
    }
    let request = json!({"request_id":Uuid::new_v4()});
    let gate = main.hold_next(SEARCH_PATH);
    *held = Some(gate.clone());
    let app = context.app.clone();
    let path = context.path();
    let headers = context.owner.headers();
    let pending = request.clone();
    let started = Instant::now();
    requests.spawn(async move { account::exchange(&app, "POST", &path, pending, headers).await });
    assert!(gate.entered_within(Duration::from_secs(1)).await);
    assert!(!gate.has_retired());
    assert!(requests.try_join_next().is_none());
    main.on(SEARCH_PATH, |request| {
        assert!(
            request
                .headers
                .get("authorization")
                .is_some_and(|header| header == format!("Bearer {OWNER_TOKEN}").as_str())
        );
        assert_eq!(request.json()["query"], "Owned newer pressure lyric query");
        let page = SearchPage {
            items: Vec::new(),
            next_cursor: None,
        };
        assert!(page.validate(20).is_ok());
        Reply::Json(serde_json::to_value(page).unwrap())
    });
    let (status, headers, success) = account::exchange(&context.app, "POST",
        &format!("/api/v1/recordings/{}/lyrics/candidates", context.owned.recording), json!({
            "installation_id":context.installation["id"], "installation_revision":context.installation["revision"],
            "query":"Owned newer pressure lyric query",
        }), context.owner.headers()).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(headers["cache-control"], "private, no-store");
    assert_eq!(success, json!({"items":[],"next_cursor":null}));
    assert_eq!(main.requests_for(SEARCH_PATH).len(), 2);
    assert_eq!(context.settings().await, owner_before);
    for plugin in others.iter() {
        plugin.on(HEALTH_PATH, |_| Reply::Status(StatusCode::UNAUTHORIZED));
    }
    main.on(HEALTH_PATH, |request| {
        if request
            .headers
            .get("authorization")
            .is_some_and(|header| header == format!("Bearer {OWNER_TOKEN}").as_str())
        {
            Reply::Json(serde_json::to_value(HealthReport::ok()).unwrap())
        } else {
            Reply::Status(StatusCode::UNAUTHORIZED)
        }
    });
    let received = join_all(
        peers
            .iter()
            .map(|peer| fixture::settings_for(&context, &peer.account)),
    )
    .await;
    let mut generations = HashSet::new();
    for (peer, settings) in peers.iter().zip(&received) {
        assert_eq!(*settings, peer.expected);
        for installation in settings["installations"].as_array().unwrap() {
            assert_ne!(installation["id"], context.installation["id"]);
            assert!(generations.insert((
                installation["id"].as_str().unwrap().to_owned(),
                installation["revision"].as_u64().unwrap()
            )));
            assert_eq!(installation["health"]["state"], "credentials_required");
        }
    }
    assert_eq!(generations.len(), 128);
    assert_eq!(main.health_requests().len() - main_health_before, 4);
    for (plugin, before) in others.iter().zip(&other_health_before) {
        assert_eq!(plugin.health_requests().len() - before, 4);
    }
    assert!(!gate.has_retired());
    assert!(requests.try_join_next().is_none());
    assert_eq!(context.settings().await, owner_before);
    assert_eq!(main.health_requests().len() - main_health_before, 5);
    assert!(!gate.has_retired());
    assert!(requests.try_join_next().is_none());
    gate.release();
    let response = tokio::time::timeout(Duration::from_secs(3), requests.join_next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert!(gate.retired_within(Duration::from_secs(1)).await);
    context.check_resolution(&response, &request);
    context.check_replay(&request, &response.2).await;
    let owner_after = context.settings().await;
    let mut normalized = owner_after.clone();
    let installation = normalized["installations"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|item| item["id"] == context.installation["id"])
        .unwrap();
    installation["health"] =
        installation_by_id(&owner_before, &context.installation["id"])["health"].clone();
    assert_eq!(normalized, owner_before);
    for peer in &peers {
        let (status, error) = peer
            .account
            .request(&context.app, "POST", &context.path(), request.clone())
            .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert!(error.get("plugin_connection").is_none());
        let headers = peer.account.headers();
        let session = headers["cookie"]
            .to_str()
            .unwrap()
            .strip_prefix("hito_session=")
            .unwrap();
        let actor = context
            .owned
            .hito
            .identity
            .authenticate(session)
            .await
            .unwrap();
        let connections = context
            .owned
            .hito
            .plugins
            .connections(&actor)
            .await
            .unwrap();
        let actual =
            json!({"services":connections.services, "installations":connections.installations});
        let mut expected = without_health(&peer.expected);
        expected
            .as_object_mut()
            .unwrap()
            .remove("credential_storage_ready");
        assert_eq!(actual, expected);
    }
    context.owned.assert_preserved().await;
    assert_eq!(
        context.owner.get(&context.app, &identities_path).await,
        identities
    );
    assert_eq!(
        context
            .owner
            .get(
                &context.app,
                &format!(
                    "/api/v1/recordings/{}/source-choice",
                    context.owned.recording
                )
            )
            .await,
        choice
    );
    assert_eq!(main.requests().len() - main_calls_before, 2);
    assert_eq!(main.requests_for(SEARCH_PATH).len(), 2);
    assert_eq!(main.health_requests().len() - main_health_before, 5);
    assert_eq!(main.unexpected(), 0);
    for (plugin, before) in others.iter().zip(&other_health_before) {
        assert_eq!(plugin.health_requests().len() - before, 4);
        assert_eq!(plugin.requests_for(MANIFEST_PATH).len(), 4);
        assert_eq!(plugin.unexpected(), 0);
    }
    for endpoint in [
        "v1/search",
        "v1/recordings/detail",
        "v1/recordings/metadata",
        "v1/sources",
        "v1/audio",
    ] {
        assert!(previous.requests(endpoint).await.is_empty());
    }
    let serialized = serde_json::to_string(&response.2).unwrap();
    for private in [
        OWNER_TOKEN,
        "Private title override",
        "primary/owned-recording",
        main.url.as_str(),
        primary.url.as_str(),
        previous.url.as_str(),
    ] {
        assert!(!serialized.contains(private));
    }
    assert!(requests.is_empty());
    assert!(started.elapsed() < Duration::from_secs(4));
    (
        response.2,
        installation_by_id(&owner_after, &context.installation["id"]).clone(),
    )
}

fn without_health(settings: &Value) -> Value {
    let mut settings = settings.clone();
    for installation in settings["installations"].as_array_mut().unwrap() {
        installation.as_object_mut().unwrap().remove("health");
    }
    settings
}
