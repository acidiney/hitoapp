use super::{Library, fixture, legacy_recovery_preserved, provider};
use axum::http::{HeaderMap, StatusCode};
use futures_util::FutureExt;
use hito_core::test_support::hang_guard::HANG_CEILING;
use hito_testkit::held_authority_snapshot::HeldSnapshot;
use serde_json::{Value, json};
use std::{
    panic::{AssertUnwindSafe, resume_unwind},
    time::{Duration, Instant},
};
use tokio::task::JoinSet;
use tracing::instrument::WithSubscriber;
use tracing_subscriber::prelude::*;

type Response = (StatusCode, HeaderMap, Value);

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn logout_committed_during_legacy_document_postcheck_preserves_private_credential_health() {
    let provider = provider::Provider::start().await;
    let hold =
        HeldSnapshot::within_span("legacy_lyric_authority", "lyric_legacy_document_postcheck");
    let mut requests = JoinSet::new();
    let mut library = None;
    let result = tokio::time::timeout(
        HANG_CEILING,
        AssertUnwindSafe(Box::pin(exercise(
            &provider,
            &hold,
            &mut library,
            &mut requests,
        )))
        .catch_unwind(),
    )
    .await;
    hold.release();
    requests.abort_all();
    let drained = tokio::time::timeout(Duration::from_secs(3), async {
        while requests.join_next().await.is_some() {}
    })
    .await;
    provider.finish().await;
    if let Some(library) = &library {
        library.assert_original();
    }
    assert!(drained.is_ok(), "Every owned legacy HTTP task must drain");
    let (current, expected) = match result {
        Ok(Ok(value)) => value,
        Ok(Err(panic)) => resume_unwind(panic),
        Err(_) => panic!("The owned legacy snapshot workflow must settle within its deadline"),
    };
    assert_eq!(
        current, expected,
        "A legacy protected401 checked through a snapshot older than committed logout must not publish private credential health"
    );
}

async fn exercise(
    provider: &provider::Provider,
    hold: &HeldSnapshot,
    library_owner: &mut Option<Library>,
    requests: &mut JoinSet<Response>,
) -> (Value, Value) {
    let library = library_owner.insert(Library::prepare(provider).await);
    let inspector = fixture::Account::login(
        &library.app,
        "owner",
        "owned artist review administrator password",
    )
    .await;
    assert_eq!(inspector.id, library.account.id);
    let settings = library.account.get(&library.app, "/api/v1/plugins").await;
    assert_eq!(settings["installations"].as_array().unwrap().len(), 1);
    let installation = settings["installations"][0].clone();
    assert_eq!(installation["revision"], library.installation_revision);
    assert_eq!(installation["credential_set"], true);
    assert!(installation.get("health").is_none());
    let service = settings["services"]
        .as_array()
        .unwrap()
        .iter()
        .find(|service| service["id"] == installation["service_id"])
        .unwrap();
    let (status, _) = library
        .account
        .request(
            &library.app,
            "POST",
            "/api/v1/users",
            json!({
                "username":"legacy-recovery-peer", "password":"owned legacy recovery peer password",
            }),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED);
    let peer = fixture::Account::login(
        &library.app,
        "legacy-recovery-peer",
        "owned legacy recovery peer password",
    )
    .await;
    let (status, peer_installation) = peer
        .request(
            &library.app,
            "POST",
            "/api/v1/plugins",
            json!({
                "service_id":service["id"], "service_revision":service["revision"],
                "bearer_token":"owned-legacy-peer-token",
            }),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_ne!(peer_installation["id"], installation["id"]);
    let peer_before = peer.get(&library.app, "/api/v1/plugins").await;
    assert_eq!(peer_before["installations"].as_array().unwrap().len(), 1);
    assert_eq!(
        peer_before["installations"][0]["id"],
        peer_installation["id"]
    );
    assert!(peer_before["installations"][0].get("health").is_none());
    let before = library.snapshot().await;
    assert_eq!(
        before["metadata"]["selected_observation"]["id"],
        library.observation
    );
    assert_eq!(
        before["metadata"]["selected_observation"]["provider_recording_id"],
        provider::PROVIDER_ID
    );
    assert_eq!(
        before["metadata"]["title"]["personal"],
        "Owned personal title pin"
    );
    assert_eq!(before["playlist"]["entries"].as_array().unwrap().len(), 2);
    let liked = library
        .account
        .get(&library.app, "/api/v1/recordings?liked=true")
        .await;
    assert!(
        liked["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|recording| recording["id"] == library.recording)
    );
    let preserved = legacy_recovery_preserved(library, &peer).await;
    assert_eq!(
        preserved["identities"]["items"].as_array().unwrap().len(),
        1
    );
    assert_eq!(
        preserved["identities"]["items"][0]["selector"],
        json!({"kind":"observation", "observation_id":library.observation})
    );
    let providers_path = format!("{}/providers", library.lookup_path());
    let providers_before = library.account.get(&library.app, &providers_path).await;
    assert_eq!(providers_before["items"].as_array().unwrap().len(), 1);
    assert_eq!(
        providers_before["items"][0]["observation_id"],
        library.observation
    );
    assert_eq!(
        providers_before["items"][0]["installation_id"],
        installation["id"]
    );
    let ready_recording = before["catalog"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|recording| recording["available"] == true)
        .unwrap()["id"]
        .as_str()
        .unwrap();
    assert_ne!(ready_recording, library.recording);
    let ready_path = format!("/api/v1/recordings/{ready_recording}/sources");
    let ready_before = library.account.get(&library.app, &ready_path).await;
    assert!(
        ready_before
            .as_array()
            .unwrap()
            .iter()
            .any(|source| source["ready"] == true)
    );
    let manifest_credentials = [provider::TOKEN, "owned-legacy-peer-token"];
    provider
        .assert_calls_with_manifest_credentials(&manifest_credentials, 0, 0)
        .await;
    provider.reject_lyrics().await;
    let app = library.app.clone();
    let path = library.lookup_path();
    let body = library.lookup_body();
    assert_eq!(
        body,
        json!({"observation_id":library.observation,
        "installation_revision":library.installation_revision})
    );
    assert!(body.get("selector").is_none());
    let headers = library.account.headers();
    hito_testkit::logs::anchor_callsite_interest();
    let subscriber = tracing_subscriber::registry().with(hold.clone());
    requests.spawn(
        async move { fixture::exchange(&app, "POST", &path, body, headers).await }
            .with_subscriber(subscriber),
    );
    hold.entered().await;
    provider
        .assert_calls_with_manifest_credentials(&manifest_credentials, 1, 1)
        .await;
    assert!(requests.try_join_next().is_none());
    let held_at = Instant::now();
    let (status, _) = tokio::time::timeout(
        Duration::from_secs(2),
        library
            .account
            .request(&library.app, "POST", "/api/v1/logout", Value::Null),
    )
    .await
    .unwrap();
    assert_eq!(status, StatusCode::NO_CONTENT);
    hold.assert_held();
    let (status, headers, rejected) = fixture::exchange(
        &library.app,
        "GET",
        "/api/v1/plugins",
        Value::Null,
        library.account.headers(),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(headers["cache-control"], "private, no-store");
    assert!(rejected.get("plugin_connection").is_none());
    let (status, headers, during) = fixture::exchange(
        &library.app,
        "GET",
        "/api/v1/plugins",
        Value::Null,
        inspector.headers(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(headers["cache-control"], "private, no-store");
    assert_eq!(during, before["plugins"]);
    hold.assert_held();
    assert!(requests.try_join_next().is_none());
    hold.release();
    let response = tokio::time::timeout(Duration::from_secs(2), requests.join_next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    hold.assert_returned();
    assert_eq!(response.0, StatusCode::UNAUTHORIZED);
    assert_eq!(response.1["cache-control"], "private, no-store");
    for key in ["plugin_connection", "code", "document", "status"] {
        assert!(response.2.get(key).is_none());
    }
    let encoded = response.2.to_string();
    for private in [
        provider::TOKEN,
        provider::PROVIDER_ID,
        provider::TITLE,
        provider::LINE,
        "owned-legacy-peer-token",
        provider.url.as_str(),
    ] {
        assert!(!encoded.contains(private));
    }
    library.account = inspector;
    let (status, headers, after_settings) = fixture::exchange(
        &library.app,
        "GET",
        "/api/v1/plugins",
        Value::Null,
        library.account.headers(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(headers["cache-control"], "private, no-store");
    assert_eq!(after_settings["installations"].as_array().unwrap().len(), 1);
    let current = after_settings["installations"][0].clone();
    assert_eq!(current["id"], installation["id"]);
    assert_eq!(current["revision"], installation["revision"]);
    assert_eq!(current["credential_set"], true);
    assert_eq!(after_settings["services"], before["plugins"]["services"]);
    assert_eq!(peer.get(&library.app, "/api/v1/plugins").await, peer_before);
    assert_eq!(legacy_recovery_preserved(library, &peer).await, preserved);
    assert_eq!(
        library
            .account
            .get(&library.app, "/api/v1/recordings?liked=true")
            .await,
        liked
    );
    assert_eq!(
        library.account.get(&library.app, &providers_path).await,
        providers_before
    );
    assert_eq!(
        library.account.get(&library.app, &ready_path).await,
        ready_before
    );
    let mut after = library.snapshot().await;
    after["plugins"]["installations"][0]
        .as_object_mut()
        .unwrap()
        .remove("health");
    assert_eq!(after, before);
    library.assert_original();
    provider
        .assert_calls_with_manifest_credentials(&manifest_credentials, 1, 1)
        .await;
    assert!(held_at.elapsed() < Duration::from_secs(20));
    (current, installation)
}
