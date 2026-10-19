use crate::{assert_private_logs, fixture, logs, provider};
use axum::http::StatusCode;
use fixture::Library;
use futures_util::FutureExt;
use hito_core::ReadRecordingLyrics;
use serde_json::json;
use std::{
    future::pending,
    panic::{AssertUnwindSafe, resume_unwind},
    time::Duration,
};
use tokio::time::timeout;
use tracing::{Instrument, instrument::WithSubscriber};

#[tokio::test]
async fn dropping_a_polled_lyric_request_with_parent_close_events_keeps_its_dispatcher() {
    let provider = provider::Provider::start().await;
    let result = timeout(
        Duration::from_secs(30),
        AssertUnwindSafe(drop_request(&provider)).catch_unwind(),
    )
    .await;
    provider.finish().await;
    match result {
        Ok(Ok(())) => {}
        Ok(Err(panic)) => resume_unwind(panic),
        Err(_) => panic!("The owned dropped lyric workflow must finish within its deadline"),
    }
}

async fn drop_request(provider: &provider::Provider) {
    let library = Library::prepare(provider).await;
    let before = library.snapshot().await;
    let cookie = library.account.headers()["cookie"]
        .to_str()
        .unwrap()
        .to_owned();
    let actor = library
        .services
        .identity
        .authenticate(cookie.strip_prefix("hito_session=").unwrap())
        .await
        .unwrap();
    let capture = logs::Logs::default();
    let subscriber = tracing::Dispatch::new(capture.subscriber_with_close_events());
    let decoy = logs::Logs::default();
    let decoy_subscriber = tracing::Dispatch::new(decoy.subscriber_with_close_events());
    let request = || ReadRecordingLyrics {
        observation_id: library.observation.clone(),
        installation_revision: library.installation_revision,
    };
    let unpolled = library
        .services
        .catalog
        .recording_lyrics_until(&actor, &library.recording, request(), pending())
        .with_subscriber(subscriber.clone());
    tracing::dispatcher::with_default(&decoy_subscriber, || drop(unpolled));
    capture.assert_empty();
    decoy.assert_empty();
    provider.assert_lookups(0).await;

    let gate = provider.hold_lyrics().await;
    let parent = tracing::dispatcher::with_default(
        &subscriber,
        || tracing::debug_span!(target: "hito_core", "owned_lyric_parent"),
    );
    let mut lookup = Box::pin(
        library
            .services
            .catalog
            .recording_lyrics_until(&actor, &library.recording, request(), pending())
            .instrument(parent)
            .with_subscriber(subscriber.clone()),
    );
    timeout(Duration::from_secs(2), async {
        tokio::select! {
            biased;
            _ = &mut lookup => panic!("The real partial lyric response must stay held"),
            _ = gate.entered() => {}
        }
    })
    .await
    .expect("The public lyric future must read a real incomplete provider response");
    provider.assert_lookups(1).await;
    tracing::dispatcher::with_default(&decoy_subscriber, || drop(lookup));
    timeout(Duration::from_secs(2), gate.retired())
        .await
        .expect("Dropping the public future must retire the body before fixture release");
    assert!(!gate.released());
    provider.assert_lookups(1).await;
    capture.assert_core_outcomes(&["cancelled"]);
    capture.assert_span_closed("recording_lyrics_until");
    capture.assert_span_closed("owned_lyric_parent");
    decoy.assert_empty();
    assert_private_logs(&capture, &library, provider, &[], &[]);

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
    capture.assert_core_outcomes(&["cancelled", "ready"]);
    decoy.assert_empty();
    assert_private_logs(&capture, &library, provider, &["ready"], &[]);
    assert!(library.snapshot().await == before);
    library.assert_original();
}
