#[path = "fixtures/timed_lyrics_audit.rs"]
mod audit;
#[path = "timed_lyrics/authority.rs"]
mod authority;
#[path = "timed_lyrics/dropped.rs"]
mod dropped;
#[path = "fixtures/timed_lyrics.rs"]
mod fixture;
#[path = "timed_lyrics/lifecycle.rs"]
mod lifecycle;
#[path = "fixtures/timed_lyrics_logs.rs"]
mod logs;
#[path = "timed_lyrics/playback.rs"]
mod playback;
#[path = "timed_lyrics/protocol.rs"]
mod protocol;
#[path = "../../hito-core/tests/fixtures/timed_lyrics_provider.rs"]
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
        assert!(
            result
                == json!({
                    "status": "ready", "recording_id": library.recording,
                    "observation_id": library.observation, "document": provider::response(present),
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

fn assert_private_logs(
    capture: &logs::Logs,
    library: &Library,
    provider: &provider::Provider,
    outcomes: &[&str],
    extra: &[&str],
) {
    let cookie = library.account.headers()["cookie"]
        .to_str()
        .unwrap()
        .to_owned();
    let token = cookie.strip_prefix("hito_session=").unwrap();
    let session_hash = format!("{:x}", Sha256::digest(token.as_bytes()));
    let mut canaries = vec![
        provider::PROVIDER_ID,
        provider::TOKEN,
        provider::TITLE,
        provider::LINE,
        provider::RECORDING_REVISION,
        provider::DOCUMENT_REVISION,
        "owned-music-private-path-canary",
        "owned-original.wav",
        &provider.url,
        &library.recording,
        &library.observation,
        &cookie,
        token,
        &session_hash,
    ];
    canaries.extend_from_slice(extra);
    capture.assert_private(&canaries, outcomes);
}
