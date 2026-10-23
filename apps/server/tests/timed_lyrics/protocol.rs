use crate::{
    assert_private_logs,
    fixture::{self, Library},
    logs, provider,
};
use axum::http::StatusCode;
use futures_util::FutureExt;
use hito_plugin_sdk::MAX_LYRICS_RESPONSE_BYTES;
use serde_json::json;
use std::{
    panic::{AssertUnwindSafe, resume_unwind},
    time::Duration,
};
use tracing::instrument::WithSubscriber;

const WRONG_ECHO: &str = "wrong:provider/echo?private=\"canary\"";

#[tokio::test]
async fn a_wrong_provider_echo_is_unavailable_and_releases_capacity_for_exact_retry() {
    let provider = provider::Provider::start().await;
    let result = tokio::time::timeout(
        Duration::from_secs(30),
        AssertUnwindSafe(wrong_echo(&provider)).catch_unwind(),
    )
    .await;
    provider.finish().await;
    match result {
        Ok(Ok(())) => {}
        Ok(Err(panic)) => resume_unwind(panic),
        Err(_) => panic!("The owned lyric echo workflow must finish within its deadline"),
    }
}

async fn wrong_echo(provider: &provider::Provider) {
    let library = Library::prepare(provider).await;
    let before = library.snapshot().await;
    let capture = logs::Logs::default();
    let subscriber = tracing::Dispatch::new(capture.subscriber());
    let mut response = json!(provider::response(true));
    response["recording"]["id"] = json!(WRONG_ECHO);
    provider.lyrics(response).await;
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
        result == json!({"status": "unavailable"}),
        "A response for another provider recording must never return lyric content"
    );
    provider.assert_lookups(1).await;
    assert!(library.snapshot().await == before);
    provider.lyrics(json!(provider::response(true))).await;
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
    provider.assert_lookups(2).await;
    assert!(library.snapshot().await == before);
    library.assert_original();
    assert_private_logs(
        &capture,
        &library,
        provider,
        &["unavailable", "ready"],
        &[WRONG_ECHO],
    );
}

#[tokio::test]
async fn received_chunked_lyric_bytes_obey_the_exact_cap_before_decoding_and_allow_retry() {
    let provider = provider::Provider::start().await;
    let result = tokio::time::timeout(
        Duration::from_secs(30),
        AssertUnwindSafe(streamed_body_cap(&provider)).catch_unwind(),
    )
    .await;
    provider.finish().await;
    match result {
        Ok(Ok(())) => {}
        Ok(Err(panic)) => resume_unwind(panic),
        Err(_) => panic!("The owned lyric body-cap workflow must finish within its deadline"),
    }
}

async fn streamed_body_cap(provider: &provider::Provider) {
    let library = Library::prepare(provider).await;
    let before = library.snapshot().await;
    let capture = logs::Logs::default();
    let subscriber = tracing::Dispatch::new(capture.subscriber());
    for (length, expected) in [
        (MAX_LYRICS_RESPONSE_BYTES, "ready"),
        (MAX_LYRICS_RESPONSE_BYTES + 1, "unavailable"),
    ] {
        let mut bytes = serde_json::to_vec(&provider::response(true)).unwrap();
        assert!(bytes.len() < length);
        bytes.resize(length, b' ');
        provider.chunked_lyrics(bytes).await;
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
        assert!(result["status"] == expected);
        if expected == "ready" {
            assert!(result["document"] == json!(provider::response(true)));
        } else {
            assert!(result == json!({"status": "unavailable"}));
        }
        assert!(library.snapshot().await == before);
    }
    provider.lyrics(json!(provider::response(true))).await;
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
    provider.assert_lookups(3).await;
    provider
        .assert_chunked_lyrics_lengths(&[MAX_LYRICS_RESPONSE_BYTES, MAX_LYRICS_RESPONSE_BYTES + 1])
        .await;
    assert!(library.snapshot().await == before);
    library.assert_original();
    assert_private_logs(
        &capture,
        &library,
        provider,
        &["ready", "unavailable", "ready"],
        &[],
    );
}
