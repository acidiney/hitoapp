use super::providers::attached;
use super::{Library, fixture, legacy_recovery_preserved_with_mode, logs, provider};
use axum::http::{HeaderMap, StatusCode};
use futures_util::FutureExt;
use hito_core::test_support::hang_guard::HANG_CEILING;
use hito_testkit::held_authority_snapshot::HeldSnapshot;
use serde_json::{Value, json};
use std::{
    panic::{AssertUnwindSafe, resume_unwind},
    time::Duration,
};
use tokio::task::JoinSet;
use tracing::instrument::WithSubscriber;
use tracing_subscriber::prelude::*;
use uuid::Uuid;

type Response = (StatusCode, HeaderMap, Value);

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn source_observation_replaced_during_legacy_postcheck_preserves_private_credential_health() {
    let primary = provider::Provider::start().await;
    let attached = attached::Provider::start().await;
    let hold =
        HeldSnapshot::within_span("legacy_lyric_authority", "lyric_legacy_document_postcheck");
    let mut requests = JoinSet::new();
    let mut library = None;
    let result = tokio::time::timeout(
        HANG_CEILING,
        AssertUnwindSafe(Box::pin(exercise(
            &primary,
            &attached,
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
    attached.finish().await;
    primary.finish().await;
    if let Some(library) = &library {
        library.assert_original();
    }
    assert!(drained.is_ok(), "Every owned legacy HTTP task must drain");
    let (current, expected) = match result {
        Ok(Ok(value)) => value,
        Ok(Err(panic)) => resume_unwind(panic),
        Err(_) => panic!("The owned source-observation snapshot workflow must settle"),
    };
    assert_eq!(
        current, expected,
        "A legacy protected401 checked through a snapshot older than committed source-observation replacement must not publish private credential health"
    );
}

async fn exercise(
    primary: &provider::Provider,
    attached: &attached::Provider,
    hold: &HeldSnapshot,
    library_owner: &mut Option<Library>,
    requests: &mut JoinSet<Response>,
) -> (Value, Value) {
    let library = library_owner.insert(Library::prepare(primary).await);
    let primary_metadata = library
        .account
        .get(
            &library.app,
            &format!("/api/v1/recordings/{}/metadata", library.recording),
        )
        .await;
    let installation = library
        .account
        .connect(
            &library.app,
            &attached.url,
            attached::NAME,
            Some(attached::TOKEN),
        )
        .await;
    let candidate = json!({"installation_id":installation["id"],
        "installation_revision":installation["revision"],
        "provider_recording_id":attached::RECORDING});
    let base = format!("/api/v1/recordings/{}", library.recording);
    let (status, review) = library
        .account
        .request(
            &library.app,
            "POST",
            &format!("{base}/sources/review"),
            candidate.clone(),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let (status, adopted) = library
        .account
        .request(
            &library.app,
            "POST",
            &format!("{base}/sources/attach"),
            json!({"candidate":candidate,"review_stamp":review["review_stamp"]}),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(adopted["id"], library.recording);
    attached.assert_calls(2, 2, 0);

    let settings = library.account.get(&library.app, "/api/v1/plugins").await;
    let service = settings["services"]
        .as_array()
        .unwrap()
        .iter()
        .find(|service| service["id"] != installation["service_id"])
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
    let (status, _) = peer
        .request(
            &library.app,
            "POST",
            "/api/v1/plugins",
            json!({
                "service_id":service["id"],"service_revision":service["revision"],
                "bearer_token":"owned-legacy-peer-token",
            }),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED);
    let peer_before = peer.get(&library.app, "/api/v1/plugins").await;
    let before = library.snapshot().await;
    assert_eq!(
        before["metadata"]["selected_observation"],
        primary_metadata["selected_observation"]
    );
    assert_eq!(before["metadata"]["title"], primary_metadata["title"]);
    assert_eq!(before["metadata"]["artist"], primary_metadata["artist"]);
    assert_eq!(
        before["metadata"]["observations"]["next_before"],
        Value::Null
    );
    let old = before["metadata"]["observations"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["plugin_installation_id"] == installation["id"])
        .unwrap()
        .clone();
    assert_eq!(old["provider_recording_id"], attached::RECORDING);
    assert_eq!(
        old["provider_revision"],
        json!(attached.reference().revision)
    );
    let expected_installation = before["plugins"]["installations"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["id"] == installation["id"])
        .unwrap()
        .clone();
    assert!(expected_installation.get("health").is_none());
    assert_eq!(expected_installation["credential_set"], true);
    let preserved = legacy_recovery_preserved_with_mode(library, &peer, "reviewed").await;
    assert_eq!(
        preserved["identities"]["items"].as_array().unwrap().len(),
        2
    );
    let providers_path = format!("{base}/lyrics/providers");
    let providers_before = library.account.get(&library.app, &providers_path).await;
    assert_eq!(providers_before["items"].as_array().unwrap().len(), 2);
    assert_eq!(providers_before["items"][1]["observation_id"], old["id"]);
    let liked_before = library
        .account
        .get(&library.app, "/api/v1/recordings?liked=true")
        .await;
    let ready_id = before["catalog"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["available"] == true)
        .unwrap()["id"]
        .as_str()
        .unwrap();
    assert_ne!(ready_id, library.recording);
    let ready_path = format!("/api/v1/recordings/{ready_id}/sources");
    let ready_before = library.account.get(&library.app, &ready_path).await;
    assert!(
        ready_before
            .as_array()
            .unwrap()
            .iter()
            .any(|source| source["ready"] == true)
    );
    primary
        .assert_calls_with_manifest_credentials(&[provider::TOKEN, "owned-legacy-peer-token"], 0, 0)
        .await;
    attached.reject_lyrics();
    let app = library.app.clone();
    let path = library.lookup_path();
    let headers = library.account.headers();
    let body = json!({"observation_id":old["id"],
        "installation_revision":installation["revision"]});
    let capture = logs::Logs::default();
    let subscriber = capture.subscriber().with(hold.clone());
    requests.spawn(
        async move { fixture::exchange(&app, "POST", &path, body, headers).await }
            .with_subscriber(subscriber),
    );
    hold.entered().await;
    attached.assert_calls(2, 2, 1);
    assert!(requests.try_join_next().is_none());

    attached.advance();
    let (status, adopted) = tokio::time::timeout(
        Duration::from_secs(2),
        library.account.request(
            &library.app,
            "POST",
            &format!(
                "/api/v1/plugins/{}/recordings",
                installation["id"].as_str().unwrap()
            ),
            json!({"installation_revision":installation["revision"],
                "provider_recording_id":attached::RECORDING}),
        ),
    )
    .await
    .unwrap();
    assert_eq!(status, StatusCode::OK);
    assert_eq!(adopted["id"], library.recording);
    hold.assert_held();
    attached.assert_calls(3, 3, 1);
    let during = library
        .account
        .get(&library.app, &format!("{base}/metadata"))
        .await;
    let expected_metadata = replacement_metadata(&before["metadata"], &during, &old, attached);
    let new_id = expected_metadata["observations"]["items"][0]["id"].clone();
    let mut expected_providers = providers_before.clone();
    replace_reference(
        &mut expected_providers,
        &installation,
        &new_id,
        attached,
        false,
    );
    assert_eq!(
        library.account.get(&library.app, &providers_path).await,
        expected_providers
    );
    let mut expected_preserved = preserved.clone();
    replace_reference(
        &mut expected_preserved["identities"],
        &installation,
        &new_id,
        attached,
        true,
    );
    assert_eq!(
        legacy_recovery_preserved_with_mode(library, &peer, "reviewed").await,
        expected_preserved
    );
    let (status, headers, current_settings) = fixture::exchange(
        &library.app,
        "GET",
        "/api/v1/plugins",
        Value::Null,
        library.account.headers(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(headers["cache-control"], "private, no-store");
    assert_eq!(current_settings, before["plugins"]);
    let mut expected = before.clone();
    expected["metadata"] = expected_metadata;
    assert_eq!(library.snapshot().await, expected);
    assert_eq!(peer.get(&library.app, "/api/v1/plugins").await, peer_before);
    hold.assert_held();
    assert!(requests.try_join_next().is_none());
    hold.release();
    let response = tokio::time::timeout(Duration::from_secs(2), requests.join_next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    hold.assert_returned();
    assert_eq!(response.0, StatusCode::NOT_FOUND);
    assert_eq!(response.1["cache-control"], "private, no-store");
    for key in [
        "plugin_connection",
        "credential_context",
        "code",
        "document",
        "status",
    ] {
        assert!(response.2.get(key).is_none());
    }
    let encoded = response.2.to_string();
    for private in [
        attached::TOKEN,
        attached::RECORDING,
        attached::TITLE,
        attached.url.as_str(),
        "owned-legacy-peer-token",
    ] {
        assert!(!encoded.contains(private));
    }
    let (status, headers, settings) = fixture::exchange(
        &library.app,
        "GET",
        "/api/v1/plugins",
        Value::Null,
        library.account.headers(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(headers["cache-control"], "private, no-store");
    let current = settings["installations"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["id"] == installation["id"])
        .unwrap()
        .clone();
    assert_eq!(current["revision"], expected_installation["revision"]);
    assert_eq!(current["credential_set"], true);
    let mut after = library.snapshot().await;
    after["plugins"]["installations"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|item| item["id"] == installation["id"])
        .unwrap()
        .as_object_mut()
        .unwrap()
        .remove("health");
    assert_eq!(after, expected);
    assert_eq!(
        legacy_recovery_preserved_with_mode(library, &peer, "reviewed").await,
        expected_preserved
    );
    assert_eq!(
        library.account.get(&library.app, &providers_path).await,
        expected_providers
    );
    assert_eq!(
        library
            .account
            .get(&library.app, "/api/v1/recordings?liked=true")
            .await,
        liked_before
    );
    assert_eq!(
        library.account.get(&library.app, &ready_path).await,
        ready_before
    );
    assert_eq!(peer.get(&library.app, "/api/v1/plugins").await, peer_before);
    primary
        .assert_calls_with_manifest_credentials(&[provider::TOKEN, "owned-legacy-peer-token"], 0, 0)
        .await;
    attached.assert_calls(3, 3, 1);
    library.assert_original();
    super::assert_private_logs(
        &capture,
        library,
        primary,
        &["not_found"],
        &[
            attached::TOKEN,
            attached::RECORDING,
            attached::TITLE,
            attached.url.as_str(),
            old["id"].as_str().unwrap(),
            new_id.as_str().unwrap(),
        ],
    );
    (current, expected_installation)
}

fn replacement_metadata(
    before: &Value,
    current: &Value,
    old: &Value,
    attached: &attached::Provider,
) -> Value {
    let previous = before["observations"]["items"].as_array().unwrap();
    let items = current["observations"]["items"].as_array().unwrap();
    assert_eq!(items.len(), previous.len() + 1);
    assert_eq!(&items[1..], previous.as_slice());
    let observed = &items[0];
    let id = Uuid::parse_str(observed["id"].as_str().unwrap()).unwrap();
    assert!(!id.is_nil() && previous.iter().all(|item| item["id"] != observed["id"]));
    let at = observed["observed_at"].as_i64().unwrap();
    assert!(at >= old["observed_at"].as_i64().unwrap());
    let mut new = old.clone();
    new["id"] = json!(id);
    new["observed_at"] = json!(at);
    new["provider_revision"] = json!(attached.reference().revision);
    assert_ne!(new["provider_revision"], old["provider_revision"]);
    assert_eq!(observed, &new);
    let mut expected = before.clone();
    expected["observations"]["items"]
        .as_array_mut()
        .unwrap()
        .insert(0, new);
    assert_eq!(current, &expected);
    expected
}

fn replace_reference(
    listing: &mut Value,
    installation: &Value,
    observation: &Value,
    attached: &attached::Provider,
    identities: bool,
) {
    let items = listing["items"].as_array_mut().unwrap();
    let selected = items
        .iter_mut()
        .filter(|item| item["installation_id"] == installation["id"])
        .collect::<Vec<_>>();
    assert_eq!(selected.len(), 1);
    for item in selected {
        assert_eq!(item["installation_revision"], installation["revision"]);
        if identities {
            item["selector"] = json!({"kind":"observation", "observation_id":observation});
        } else {
            item["observation_id"] = observation.clone();
        }
        item["provider_recording"] = json!(attached.reference());
    }
}
