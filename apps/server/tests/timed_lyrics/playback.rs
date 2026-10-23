use crate::{
    assert_private_logs,
    fixture::{self, Library},
    logs, provider, source_provider,
};
use axum::{
    Router,
    body::{Body, to_bytes},
    http::{HeaderMap, Request, StatusCode},
};
use futures_util::FutureExt;
use serde_json::{Value, json};
use std::{
    panic::{AssertUnwindSafe, resume_unwind},
    time::Duration,
};
use tokio::{task::JoinSet, time::timeout};
use tower::ServiceExt;
use tracing::instrument::WithSubscriber;

type Response = (StatusCode, HeaderMap, Value);

#[tokio::test]
async fn held_lyrics_do_not_block_same_account_playback_source_refresh_or_audio() {
    let provider = provider::Provider::start().await;
    let mut source = source_provider::Provider::start("lyric-isolation", 0).await;
    let mut reads = JoinSet::new();
    let result = timeout(
        Duration::from_secs(30),
        AssertUnwindSafe(playback(&provider, &source, &mut reads)).catch_unwind(),
    )
    .await;
    reads.shutdown().await;
    provider.finish().await;
    let joined = source.finish().await;
    match result {
        Ok(Ok(())) => {}
        Ok(Err(panic)) => resume_unwind(panic),
        Err(_) => {
            panic!("The owned lyric/playback isolation workflow must finish within its deadline")
        }
    }
    assert!(
        joined,
        "The owned audio provider must close and join shutdown"
    );
}

async fn playback(
    provider: &provider::Provider,
    source: &source_provider::Provider,
    reads: &mut JoinSet<Response>,
) {
    let library = Library::prepare(provider).await;
    let installation = library
        .account
        .connect(
            &library.app,
            &source.url,
            "Owned lyric isolation audio",
            None,
        )
        .await;
    let (status, adopted) = library.account.request(&library.app, "POST", &format!(
        "/api/v1/plugins/{}/recordings", installation["id"].as_str().unwrap()
    ), json!({"installation_revision": installation["revision"], "provider_recording_id": source.recording_id()})).await;
    assert_eq!(status, StatusCode::OK);
    let recording = adopted["id"].as_str().unwrap().to_owned();
    let sources_path = format!("/api/v1/recordings/{recording}/sources");
    let old = library.account.get(&library.app, &sources_path).await;
    assert!(old.as_array().unwrap().len() == 1 && old[0]["ready"] == true);
    assert_eq!(source.requests(), 3);
    assert_eq!(source.audio_requests(), 0);
    assert_eq!(source.unexpected(), 0);
    let before = library.snapshot().await;
    source.replace_unavailable();
    source.make_ready();
    let capture = logs::Logs::default();
    let subscriber = tracing::Dispatch::new(capture.subscriber());
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
        .expect("The lyric provider must hold a real partial response before playback work");
    provider.assert_lookups(1).await;
    let (status, _, refreshed) = timeout(
        Duration::from_secs(2),
        fixture::exchange(
            &library.app,
            "POST",
            &format!("{sources_path}/refresh"),
            json!({"purpose": "playback"}),
            library.account.headers(),
        )
        .with_subscriber(subscriber.clone()),
    )
    .await
    .expect("Playback source recovery must complete while the lyric body remains held");
    assert_eq!(
        status,
        StatusCode::OK,
        "A held lyric read must not take playback source admission"
    );
    assert_eq!(
        source.requests(),
        4,
        "Playback refresh must reach the real provider source boundary"
    );
    assert!(
        refreshed
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item["recording_id"] == recording
                && item["ready"] == true
                && item["id"] != old[0]["id"]),
        "The provider's replacement representation must be published before lyrics finish"
    );
    let ready = refreshed
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["ready"] == true)
        .unwrap();
    let source_id = ready["id"].as_str().unwrap();
    assert!(!gate.released() && reads.try_join_next().is_none());
    assert_eq!(source.audio_requests(), 0);
    relay(
        &library.app,
        library.account.headers(),
        source_id,
        source.audio(),
    )
    .with_subscriber(subscriber)
    .await;
    assert!(!gate.released() && reads.try_join_next().is_none());
    assert_eq!(source.requests(), 5);
    assert_eq!(source.audio_requests(), 1);
    assert_eq!(source.unexpected(), 0);
    let (status, _) = library
        .account
        .request(
            &library.app,
            "GET",
            &format!("/api/v1/recordings/{recording}/download"),
            Value::Null,
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    provider.assert_lookups(1).await;
    provider.release_lyrics().await;
    let (status, _, result) = timeout(Duration::from_secs(2), reads.join_next())
        .await
        .expect("The lyric lookup must finish after its explicit fixture release")
        .expect("The lyric lookup must remain owned")
        .expect("The lyric lookup must not panic");
    assert_eq!(status, StatusCode::OK);
    assert!(result["status"] == "ready" && result["document"] == json!(provider::response(true)));
    assert!(
        library.snapshot().await == before,
        "Lyric lookup and the source-only replacement must preserve catalog metadata and personal collection state"
    );
    library.assert_original();
    assert_private_logs(
        &capture,
        &library,
        provider,
        &["ready"],
        &[
            &source.url,
            &source.recording_id(),
            &source.source_id(),
            &recording,
            source_id,
            source_provider::TITLE,
            source_provider::ARTIST,
        ],
    );
}

async fn relay(app: &Router, mut headers: HeaderMap, source: &str, expected: &[u8]) {
    headers.insert("range", "bytes=44-107".parse().unwrap());
    let mut request = Request::get(format!("/api/v1/sources/{source}/audio"))
        .body(Body::empty())
        .unwrap();
    *request.headers_mut() = headers;
    timeout(Duration::from_secs(2), async {
        let response = app.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::PARTIAL_CONTENT);
        assert!(response.headers()["content-type"] == "audio/wav");
        assert!(response.headers()["content-range"] == format!("bytes 44-107/{}", expected.len()));
        let body = to_bytes(response.into_body(), 65_536).await.unwrap();
        assert!(
            body.as_ref() == &expected[44..108],
            "Playback must relay the exact owned WAV byte range"
        );
    })
    .await
    .expect("Actual same-account audio relay must complete while lyrics remain held");
}
