#[path = "fixtures/timed_lyrics_audit.rs"]
mod audit;
#[path = "timed_lyrics/authority.rs"]
mod authority;
#[path = "timed_lyrics/digest.rs"]
mod digest;
#[path = "fixtures/timed_lyrics.rs"]
mod fixture;
#[path = "timed_lyrics/lifecycle.rs"]
mod lifecycle;
#[allow(dead_code)]
#[path = "fixtures/timed_lyrics_logs.rs"]
mod logs;
#[path = "timed_lyrics/playback.rs"]
mod playback;
#[path = "fixtures/timed_lyrics_privacy.rs"]
mod privacy;
#[path = "timed_lyrics/protocol.rs"]
mod protocol;
#[path = "fixtures/timed_lyrics_provider.rs"]
mod provider;
#[path = "timed_lyrics/provider_authority.rs"]
mod provider_authority;
#[path = "timed_lyrics/providers.rs"]
mod providers;
#[path = "timed_lyrics/scoped_logging.rs"]
mod scoped_logging;
#[allow(dead_code)]
#[path = "fixtures/manual_source.rs"]
mod source_provider;
#[path = "api/support.rs"]
#[allow(dead_code)]
mod support;

use axum::http::StatusCode;
use fixture::Library;
use futures_util::FutureExt;
use hito_plugin_sdk::{LyricsRequest, LyricsResponse};
use privacy::assert_private_logs;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{
    panic::{AssertUnwindSafe, resume_unwind},
    time::Duration,
};
use tracing::instrument::WithSubscriber;

#[tokio::test]
async fn an_account_reads_exact_retained_lyrics_and_null_without_mutating_its_library() {
    let provider = provider::Provider::start().await;
    let result = tokio::time::timeout(
        Duration::from_secs(30),
        AssertUnwindSafe(exercise(&provider)).catch_unwind(),
    )
    .await;
    provider.finish().await;
    match result {
        Ok(Ok(())) => {}
        Ok(Err(panic)) => resume_unwind(panic),
        Err(_) => panic!("The owned lyric lookup workflow must finish within its deadline"),
    }
}

async fn exercise(provider: &provider::Provider) {
    let library = Library::prepare(provider).await;
    provider.assert_lookups(0).await;
    let before = library.snapshot().await;
    for present in [true, false] {
        if !present {
            provider.negative().await;
        }
        let (status, headers, result) = fixture::exchange(
            &library.app,
            "POST",
            &library.lookup_path(),
            library.lookup_body(),
            library.account.headers(),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert!(
            headers
                .get("cache-control")
                .unwrap()
                .to_str()
                .unwrap()
                .contains("no-store")
        );
        let expected_document = provider::response(present);
        let expected_digest = format!(
            "{:x}",
            Sha256::digest(serde_json::to_vec(&expected_document).unwrap())
        );
        assert!(
            result
                == json!({
                    "status": "ready", "recording_id": library.recording,
                    "observation_id": library.observation, "document_digest": expected_digest,
                    "document": expected_document,
                }),
            "The host must return only the exact retained recording's validated lyric result"
        );
        let document: LyricsResponse = serde_json::from_value(result["document"].clone()).unwrap();
        assert!(
            document
                .validate_for(&LyricsRequest {
                    recording: provider::reference()
                })
                .is_ok()
        );
        assert!(
            library.snapshot().await == before,
            "Lyric lookup must preserve the public library, private collection and plugin state"
        );
        library.assert_original();
        provider.assert_lookups(if present { 1 } else { 2 }).await;
    }
}

#[tokio::test]
async fn unsupported_lyric_versions_preserve_the_approved_connection_and_allow_retry() {
    let provider = provider::Provider::start().await;
    let result = tokio::time::timeout(
        Duration::from_secs(30),
        AssertUnwindSafe(unsupported_version(&provider)).catch_unwind(),
    )
    .await;
    provider.finish().await;
    match result {
        Ok(Ok(())) => {}
        Ok(Err(panic)) => resume_unwind(panic),
        Err(_) => panic!("The owned lyric negotiation workflow must finish within its deadline"),
    }
}

async fn unsupported_version(provider: &provider::Provider) {
    let library = Library::prepare(provider).await;
    let before = library.snapshot().await;
    let capture = logs::Logs::default();
    let subscriber = tracing::Dispatch::new(capture.subscriber());
    provider.descriptor(json!({"lyrics_version": 2})).await;
    let (status, _, result) = fixture::exchange(
        &library.app,
        "POST",
        &library.lookup_path(),
        library.lookup_body(),
        library.account.headers(),
    )
    .with_subscriber(subscriber.clone())
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        result == json!({"status": "unsupported"}),
        "An unsupported extension version must not issue a lyric POST"
    );
    provider.assert_calls(1, 0).await;
    assert!(
        library.snapshot().await == before,
        "Unsupported lyrics must preserve the approved Playlists capability and all personal state"
    );
    provider.descriptor(json!({"lyrics_version": 1})).await;
    let (status, _, result) = fixture::exchange(
        &library.app,
        "POST",
        &library.lookup_path(),
        library.lookup_body(),
        library.account.headers(),
    )
    .with_subscriber(subscriber)
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(result["status"] == "ready" && result["document"] == json!(provider::response(true)));
    provider.assert_calls(2, 1).await;
    assert!(library.snapshot().await == before);
    library.assert_original();
    assert_private_logs(&capture, &library, provider, &["unsupported", "ready"], &[]);
}
#[tokio::test]
async fn a_checked_legacy_document_success_clears_only_its_owners_prior_credential_rejection() {
    let provider = provider::Provider::start().await;
    let result = tokio::time::timeout(
        Duration::from_secs(30),
        AssertUnwindSafe(exercise_legacy_credential_recovery(&provider)).catch_unwind(),
    )
    .await;
    provider.finish().await;
    let installation = match result {
        Ok(Ok(installation)) => installation,
        Ok(Err(panic)) => resume_unwind(panic),
        Err(_) => panic!("The owned legacy credential recovery must finish within its deadline"),
    };
    assert_eq!(
        installation.get("health"),
        None,
        "A validated legacy lyric document must clear its owner's older credential rejection"
    );
}

async fn exercise_legacy_credential_recovery(provider: &provider::Provider) -> serde_json::Value {
    let library = Library::prepare(provider).await;
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
    let (status, _) = library.account.request(
        &library.app, "POST", "/api/v1/users",
        json!({"username":"legacy-recovery-peer","password":"owned legacy recovery peer password"}),
    ).await;
    assert_eq!(status, StatusCode::CREATED);
    let peer = fixture::Account::login(
        &library.app,
        "legacy-recovery-peer",
        "owned legacy recovery peer password",
    )
    .await;
    let (status, peer_installation) = peer.request(
        &library.app, "POST", "/api/v1/plugins",
        json!({"service_id":service["id"],"service_revision":service["revision"],"bearer_token":"owned-legacy-peer-token"}),
    ).await;
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
        before["metadata"]["title"]["personal"],
        "Owned personal title pin"
    );
    let preserved = legacy_recovery_preserved(&library, &peer).await;
    assert_eq!(
        preserved["identities"]["items"].as_array().unwrap().len(),
        1
    );
    assert_eq!(
        preserved["identities"]["items"][0]["selector"],
        json!({
            "kind":"observation","observation_id":library.observation,
        })
    );
    let manifest_credentials = [provider::TOKEN, "owned-legacy-peer-token"];
    provider
        .assert_calls_with_manifest_credentials(&manifest_credentials, 0, 0)
        .await;
    provider.reject_lyrics().await;
    let (status, headers, failure) = fixture::exchange(
        &library.app,
        "POST",
        &library.lookup_path(),
        library.lookup_body(),
        library.account.headers(),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(headers["cache-control"], "private, no-store");
    assert_eq!(failure["code"], "plugin_credentials_required");
    assert_eq!(
        failure["plugin_connection"],
        json!({
            "installation_id":installation["id"],"installation_revision":installation["revision"],
        })
    );
    assert_eq!(failure.as_object().unwrap().len(), 3);
    assert!(
        failure["error"]
            .as_str()
            .is_some_and(|text| !text.is_empty() && text.len() <= 240)
    );
    for private in [
        provider::TOKEN,
        provider::PROVIDER_ID,
        provider::TITLE,
        provider::LINE,
        "owned-legacy-peer-token",
    ] {
        assert!(!failure.to_string().contains(private));
    }
    let rejected_at = std::time::Instant::now();
    let diagnosed = library.account.get(&library.app, "/api/v1/plugins").await;
    assert_eq!(
        diagnosed["installations"][0]["health"]["state"],
        "credentials_required"
    );
    provider
        .assert_calls_with_manifest_credentials(&manifest_credentials, 1, 1)
        .await;
    provider.lyrics(json!(provider::response(true))).await;
    let (status, headers, response) = fixture::exchange(
        &library.app,
        "POST",
        &library.lookup_path(),
        library.lookup_body(),
        library.account.headers(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(headers["cache-control"], "private, no-store");
    let document = provider::response(true);
    assert!(
        document
            .validate_for(&LyricsRequest {
                recording: provider::reference()
            })
            .is_ok()
    );
    let digest = format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(&document).unwrap())
    );
    assert_eq!(
        response,
        json!({
            "status":"ready","recording_id":library.recording,"observation_id":library.observation,
            "document_digest":digest,"document":document,
        })
    );
    let (status, headers, recovered) = fixture::exchange(
        &library.app,
        "GET",
        "/api/v1/plugins",
        serde_json::Value::Null,
        library.account.headers(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(headers["cache-control"], "private, no-store");
    assert_eq!(recovered["installations"].as_array().unwrap().len(), 1);
    let current = recovered["installations"][0].clone();
    assert_eq!(current["id"], installation["id"]);
    assert_eq!(current["revision"], installation["revision"]);
    assert_eq!(current["credential_set"], true);
    assert_eq!(recovered["services"], settings["services"]);
    assert_eq!(peer.get(&library.app, "/api/v1/plugins").await, peer_before);
    assert_eq!(legacy_recovery_preserved(&library, &peer).await, preserved);
    let mut after = library.snapshot().await;
    after["plugins"]["installations"][0]
        .as_object_mut()
        .unwrap()
        .remove("health");
    assert_eq!(after, before);
    library.assert_original();
    provider
        .assert_calls_with_manifest_credentials(&manifest_credentials, 2, 2)
        .await;
    assert!(rejected_at.elapsed() < Duration::from_secs(20));
    current
}

async fn legacy_recovery_preserved(
    library: &Library,
    peer: &fixture::Account,
) -> serde_json::Value {
    legacy_recovery_preserved_with_mode(library, peer, "automatic").await
}

async fn legacy_recovery_preserved_with_mode(
    library: &Library,
    peer: &fixture::Account,
    mode: &str,
) -> serde_json::Value {
    let base = format!("/api/v1/recordings/{}", library.recording);
    let identities = library
        .account
        .get(&library.app, &format!("{base}/lyrics/identities"))
        .await;
    let (owner_status, owner_choice) = library
        .account
        .request(
            &library.app,
            "GET",
            &format!("{base}/source-choice"),
            serde_json::Value::Null,
        )
        .await;
    assert_eq!(owner_status, StatusCode::OK);
    assert_eq!(
        owner_choice,
        json!({
            "recording_id":library.recording,"revision":0,"mode":mode,"sources":[],
        })
    );
    let (peer_status, peer_identities) = peer
        .request(
            &library.app,
            "GET",
            &format!("{base}/lyrics/identities"),
            serde_json::Value::Null,
        )
        .await;
    assert_eq!(peer_status, StatusCode::NOT_FOUND);
    for response in [&owner_choice, &peer_identities] {
        assert!(response.get("plugin_connection").is_none());
        let encoded = response.to_string();
        for private in [
            provider::TOKEN,
            provider::PROVIDER_ID,
            provider::TITLE,
            provider::LINE,
            "owned-legacy-peer-token",
        ] {
            assert!(!encoded.contains(private));
        }
    }
    json!({"identities":identities,"owner_choice":owner_choice,"peer_identities":peer_identities})
}

#[path = "timed_lyrics/legacy_authority_snapshot.rs"]
mod legacy_authority_snapshot;

#[path = "timed_lyrics/legacy_source_observation_snapshot.rs"]
mod legacy_source_observation_snapshot;
