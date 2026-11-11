use super::setup::Prepared;
use crate::{account, lyric_provider};
use hito_core::{ReadSelectedRecordingLyrics, SelectedLyricsLookup};
use serde_json::json;
use std::time::{Duration, Instant};

async fn actor(prepared: &Prepared) -> hito_core::Actor {
    let headers = prepared.library.account.headers();
    let cookie = headers["cookie"].to_str().unwrap();
    prepared
        .library
        .services
        .identity
        .authenticate(cookie.strip_prefix("hito_session=").unwrap())
        .await
        .unwrap()
}

#[tokio::test]
async fn cancellation_and_dropped_http_requests_close_peer_bodies_and_release_the_original_admission()
 {
    let prepared = Prepared::new().await;
    let actor = actor(&prepared).await;
    let gate = prepared.provider.hold("v2_descriptor");
    let catalog = prepared.library.services.catalog.clone();
    let recording = prepared.library.recording.clone();
    let task_actor = actor.clone();
    let request: ReadSelectedRecordingLyrics =
        serde_json::from_value(prepared.read_body()).unwrap();
    let (cancel, receive) = tokio::sync::oneshot::channel();
    let task = tokio::spawn(async move {
        catalog
            .selected_recording_lyrics_until(&task_actor, &recording, request, async {
                let _ = receive.await;
            })
            .await
    });
    tokio::time::timeout(Duration::from_secs(3), gate.entered())
        .await
        .unwrap();
    let duplicate = prepared
        .library
        .services
        .catalog
        .selected_recording_lyrics_until(
            &actor,
            &prepared.library.recording,
            serde_json::from_value(prepared.read_body()).unwrap(),
            std::future::pending(),
        )
        .await;
    assert!(
        matches!(duplicate, Err(hito_core::Error::Conflict(_))),
        "Negotiation and body reads must keep the same per-account admission"
    );
    cancel.send(()).unwrap();
    let result = tokio::time::timeout(Duration::from_secs(2), task)
        .await
        .unwrap()
        .unwrap();
    assert!(matches!(result, Ok(SelectedLyricsLookup::Cancelled)));
    tokio::time::timeout(Duration::from_secs(2), gate.retired())
        .await
        .expect("Explicit cancellation must close the descriptor body");
    assert!(!gate.released());
    prepared.provider.release();
    prepared
        .provider
        .lyrics(lyric_provider::response(
            json!({"kind":"plain","lines":["Owned fresh result after cancellation"]}),
        ))
        .await;
    let result = prepared.read().await;
    assert_eq!(
        result["document"]["lyrics"]["content"]["lines"],
        json!(["Owned fresh result after cancellation"])
    );
    let gate = prepared.provider.hold("v2_lyrics");
    let app = prepared.library.app.clone();
    let path = format!("{}/identities/read", prepared.base());
    let body = prepared.read_body();
    let headers = prepared.library.account.headers();
    let http =
        tokio::spawn(async move { account::exchange(&app, "POST", &path, body, headers).await });
    tokio::time::timeout(Duration::from_secs(3), gate.entered())
        .await
        .unwrap();
    http.abort();
    assert!(http.await.unwrap_err().is_cancelled());
    tokio::time::timeout(Duration::from_secs(2), gate.retired())
        .await
        .expect("Dropping the HTTP request must close its lyric body");
    assert!(!gate.released());
    prepared.provider.release();
    assert_eq!(prepared.read().await["status"], "ready_v2");
    prepared.provider.assert_calls(&[
        "manifest",
        "metadata",
        "metadata",
        "v2_descriptor",
        "v2_descriptor",
        "v2_lyrics",
        "v2_descriptor",
        "v2_lyrics",
        "v2_descriptor",
        "v2_lyrics",
    ]);
    prepared.library.assert_original();
    prepared.provider.finish().await;
}

#[tokio::test]
async fn v2_negotiation_and_v1_fallback_share_the_original_total_deadline() {
    let prepared = Prepared::new().await;
    prepared
        .provider
        .descriptor(Some(json!({"lyrics_version":3})))
        .await;
    let first = prepared.provider.hold("v2_descriptor");
    let actor = actor(&prepared).await;
    let catalog = prepared.library.services.catalog.clone();
    let recording = prepared.library.recording.clone();
    let request = serde_json::from_value(prepared.read_body()).unwrap();
    let started = Instant::now();
    let task = tokio::spawn(async move {
        catalog
            .selected_recording_lyrics_until(&actor, &recording, request, std::future::pending())
            .await
    });
    tokio::time::timeout(Duration::from_secs(3), first.entered())
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_secs(6)).await;
    prepared.provider.release();
    let second = prepared.provider.hold("v1_lyrics");
    tokio::time::timeout(Duration::from_secs(2), second.entered())
        .await
        .unwrap();
    let result = tokio::time::timeout(Duration::from_secs(5), task)
        .await
        .expect("Fallback must not receive a fresh ten-second lifetime")
        .unwrap();
    assert!(matches!(result, Ok(SelectedLyricsLookup::Unavailable)));
    assert!(started.elapsed() < Duration::from_secs(11));
    tokio::time::timeout(Duration::from_secs(2), second.retired())
        .await
        .expect("The total deadline must close the fallback body");
    assert!(!second.released());
    prepared.provider.assert_calls(&[
        "manifest",
        "metadata",
        "metadata",
        "v2_descriptor",
        "v1_descriptor",
        "v1_lyrics",
    ]);
    prepared.library.assert_original();
    prepared.provider.finish().await;
}
