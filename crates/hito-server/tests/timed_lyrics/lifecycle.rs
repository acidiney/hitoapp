use crate::{
    assert_private_logs,
    fixture::{self, Account, Library},
    logs, provider,
};
use axum::http::{HeaderMap, StatusCode};
use futures_util::FutureExt;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    panic::{AssertUnwindSafe, resume_unwind},
    time::Duration,
};
use tokio::{task::JoinSet, time::timeout};
use tracing::instrument::WithSubscriber;

type Response = (StatusCode, HeaderMap, Value);

#[tokio::test]
async fn session_revocation_retires_a_held_lyric_body_before_same_account_recovery() {
    let provider = provider::Provider::start().await;
    let mut reads = JoinSet::new();
    let result = timeout(
        Duration::from_secs(30),
        AssertUnwindSafe(revoke_session(&provider, &mut reads)).catch_unwind(),
    )
    .await;
    reads.shutdown().await;
    provider.finish().await;
    match result {
        Ok(Ok(())) => {}
        Ok(Err(panic)) => resume_unwind(panic),
        Err(_) => panic!("The owned lyric revocation workflow must finish within its deadline"),
    }
}

async fn revoke_session(provider: &provider::Provider, reads: &mut JoinSet<Response>) {
    let mut library = Library::prepare(provider).await;
    let before = library.snapshot().await;
    let capture = logs::Logs::default();
    let subscriber = tracing::Dispatch::new(capture.subscriber());
    let old_cookie = library.account.headers()["cookie"]
        .to_str()
        .unwrap()
        .to_owned();
    let old_token = old_cookie.strip_prefix("hito_session=").unwrap();
    let old_hash = format!("{:x}", Sha256::digest(old_token.as_bytes()));
    let gate = provider.hold_lyrics().await;
    let app = library.app.clone();
    let path = library.lookup_path();
    let body = library.lookup_body();
    let headers = library.account.headers();
    reads.spawn(
        async move { fixture::exchange(&app, "POST", &path, body, headers).await }
            .with_subscriber(subscriber.clone()),
    );
    timeout(Duration::from_secs(2), gate.entered())
        .await
        .expect("The provider must yield an incomplete JSON body before revocation");
    provider.assert_lookups(1).await;
    let (status, _, _) = fixture::exchange(
        &library.app,
        "POST",
        &library.lookup_path(),
        library.lookup_body(),
        library.account.headers(),
    )
    .with_subscriber(subscriber.clone())
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    provider.assert_lookups(1).await;
    let (status, _) = library
        .account
        .request(&library.app, "POST", "/api/v1/logout", Value::Null)
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, _, result) = timeout(Duration::from_secs(2), reads.join_next())
        .await
        .expect("A revoked session must finish while the upstream lyric body is held")
        .expect("The in-flight lyric lookup must remain owned")
        .expect("The revoked lyric lookup must not panic");
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert!(result.get("document").is_none());
    timeout(Duration::from_secs(2), gate.retired())
        .await
        .expect("Revocation must drop the real upstream body before release or shutdown");
    assert!(
        !gate.released(),
        "Body retirement must be caused by host authorization, not fixture release"
    );
    provider.assert_lookups(1).await;
    provider.release_lyrics().await;
    let owner = library.account.id.clone();
    library.account = Account::login(
        &library.app,
        "owner",
        "owned artist review administrator password",
    )
    .await;
    assert!(library.account.id == owner);
    assert!(
        library.snapshot().await == before,
        "Session revocation must preserve retained identities, original collection state and provider approval"
    );
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
        &["conflict", "unauthorized", "ready"],
        &[&old_cookie, old_token, &old_hash],
    );
}

#[tokio::test]
async fn descriptor_and_lyric_body_share_one_total_deadline_and_release_retry_capacity() {
    let provider = provider::Provider::start().await;
    let mut reads = JoinSet::new();
    let result = timeout(
        Duration::from_secs(30),
        AssertUnwindSafe(one_total_budget(&provider, &mut reads)).catch_unwind(),
    )
    .await;
    reads.shutdown().await;
    provider.finish().await;
    match result {
        Ok(Ok(())) => {}
        Ok(Err(panic)) => resume_unwind(panic),
        Err(_) => panic!("The owned total lyric deadline workflow must finish within its deadline"),
    }
}

async fn one_total_budget(provider: &provider::Provider, reads: &mut JoinSet<Response>) {
    let library = Library::prepare(provider).await;
    let before = library.snapshot().await;
    let capture = logs::Logs::default();
    let subscriber = tracing::Dispatch::new(capture.subscriber());
    let descriptor = provider.hold_descriptor().await;
    let lyrics = provider.hold_lyrics().await;
    let app = library.app.clone();
    let path = library.lookup_path();
    let body = library.lookup_body();
    let headers = library.account.headers();
    let started = std::time::Instant::now();
    reads.spawn(
        async move { fixture::exchange_lyrics_deadline(&app, &path, body, headers).await }
            .with_subscriber(subscriber.clone()),
    );
    timeout(Duration::from_secs(2), descriptor.entered())
        .await
        .expect("The provider descriptor must yield an incomplete JSON body");
    provider.assert_calls(1, 0).await;
    assert!(
        timeout(Duration::from_secs(6), reads.join_next())
            .await
            .is_err(),
        "The owned descriptor must remain held before its explicit release"
    );
    assert!(!descriptor.released());
    provider.release_descriptor().await;
    timeout(Duration::from_secs(2), lyrics.entered())
        .await
        .expect("The remaining total budget must include the real lyric body");
    provider.assert_lookups(1).await;
    let (status, _, result) = timeout(Duration::from_secs(6), reads.join_next()).await
        .expect("The total descriptor-plus-body budget must expire near10s, before a separate body deadline could expire")
        .expect("The deadline lookup must remain owned").expect("The deadline lookup must not panic");
    assert_eq!(status, StatusCode::OK);
    assert!(result == json!({"status": "unavailable"}));
    let elapsed = started.elapsed();
    assert!((Duration::from_millis(9500)..=Duration::from_millis(12500)).contains(&elapsed));
    eprintln!("lyric_total_budget elapsed_ms={}", elapsed.as_millis());
    timeout(Duration::from_secs(2), lyrics.retired())
        .await
        .expect("The total deadline must retire the actual upstream body before fixture release");
    assert!(!lyrics.released());
    provider.release_lyrics().await;
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
    assert_private_logs(&capture, &library, provider, &["unavailable", "ready"], &[]);
}

#[tokio::test]
async fn explicit_core_cancellation_retires_the_body_and_releases_same_account_capacity() {
    let provider = provider::Provider::start().await;
    let mut reads = JoinSet::new();
    let result = timeout(
        Duration::from_secs(30),
        AssertUnwindSafe(explicit_cancel(&provider, &mut reads)).catch_unwind(),
    )
    .await;
    reads.shutdown().await;
    provider.finish().await;
    match result {
        Ok(Ok(())) => {}
        Ok(Err(panic)) => resume_unwind(panic),
        Err(_) => {
            panic!("The owned explicit lyric cancellation workflow must finish within its deadline")
        }
    }
}

async fn explicit_cancel(
    provider: &provider::Provider,
    reads: &mut JoinSet<hito_core::Result<hito_core::LyricsLookup>>,
) {
    use hito_core::{LyricsLookup, ReadRecordingLyrics};
    let library = Library::prepare(provider).await;
    let before = library.snapshot().await;
    let cookie = library.account.headers()["cookie"]
        .to_str()
        .unwrap()
        .to_owned();
    let token = cookie.strip_prefix("hito_session=").unwrap();
    let actor = library.services.identity.authenticate(token).await.unwrap();
    let capture = logs::Logs::default();
    let subscriber = tracing::Dispatch::new(capture.subscriber());
    let gate = provider.hold_lyrics().await;
    let (cancel, cancelled) = tokio::sync::oneshot::channel();
    let catalog = library.services.catalog.clone();
    let recording = library.recording.clone();
    let request = ReadRecordingLyrics {
        observation_id: library.observation.clone(),
        installation_revision: library.installation_revision,
    };
    reads.spawn(
        async move {
            catalog
                .recording_lyrics_until(&actor, &recording, request, async move {
                    let _ = cancelled.await;
                })
                .await
        }
        .with_subscriber(subscriber.clone()),
    );
    timeout(Duration::from_secs(2), gate.entered())
        .await
        .expect("Explicit cancellation must follow a real authorized partial lyric response");
    provider.assert_lookups(1).await;
    cancel
        .send(())
        .expect("The explicit cancellation future must still be owned");
    let result = timeout(Duration::from_secs(2), reads.join_next())
        .await
        .expect("Explicit cancellation must finish while the provider body stays held")
        .expect("The cancelled public core read must remain owned")
        .expect("The cancelled public core read must not panic");
    assert!(matches!(result, Ok(LyricsLookup::Cancelled)));
    timeout(Duration::from_secs(2), gate.retired())
        .await
        .expect("Cancellation must drop the actual upstream body before fixture release");
    assert!(!gate.released());
    provider.assert_lookups(1).await;
    provider.release_lyrics().await;
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
    assert_private_logs(&capture, &library, provider, &["ready"], &[]);
    capture.assert_core_outcome("cancelled");
}

#[test]
#[ignore = "Finite scoped-subscriber diagnostic; run once explicitly after host qualification"]
fn finite_concurrent_lifecycle_logging_keeps_every_strict_assertion() {
    let start = std::sync::Arc::new(std::sync::Barrier::new(3));
    std::thread::scope(|scope| {
        let mut workers = Vec::new();
        let workflows: [fn(); 3] = [
            super::unsupported_lyric_versions_preserve_the_approved_connection_and_allow_retry,
            session_revocation_retires_a_held_lyric_body_before_same_account_recovery,
            explicit_core_cancellation_retires_the_body_and_releases_same_account_capacity,
        ];
        for workflow in workflows {
            let start = start.clone();
            workers.push(scope.spawn(move || {
                start.wait();
                for _ in 0..4 {
                    workflow();
                }
            }));
        }
        for worker in workers {
            if let Err(panic) = worker.join() {
                resume_unwind(panic);
            }
        }
    });
}
