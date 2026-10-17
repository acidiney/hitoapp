#[path = "fixtures/resolution_musical.rs"]
#[allow(dead_code)]
mod fixture;
mod support;

use futures_util::FutureExt;
use hito_core::{ResolveSource, SourceResolutionOutcome};
use hito_plugin_sdk::{ProviderRecording, RecordingMetadata, SourcePage};
use serde_json::json;
use std::{panic::AssertUnwindSafe, time::Duration};
use uuid::Uuid;

#[tokio::test]
async fn wrapped_live_at_video_title_cannot_resolve_a_studio_recording() {
    assert_wrapped_live_video_is_rejected("Owned performer - Owned tune - Live at the owned venue")
        .await;
}

#[tokio::test]
async fn wrapped_live_from_video_title_cannot_resolve_a_studio_recording() {
    assert_wrapped_live_video_is_rejected(
        "Owned performer - Owned tune - Live from the owned venue",
    )
    .await;
}

#[tokio::test]
async fn wrapped_live_in_video_title_cannot_resolve_a_studio_recording() {
    assert_wrapped_live_video_is_rejected("Owned performer - Owned tune - Live in the owned venue")
        .await;
}

async fn assert_wrapped_live_video_is_rejected(title: &str) {
    let spotify = fixture::ProviderServer::start().await;
    let youtube = fixture::ProviderServer::start().await;
    let result = tokio::time::timeout(
        Duration::from_secs(8),
        AssertUnwindSafe(async {
            let owned = fixture::SavedRecording::prepare(&spotify, &youtube).await;
            let before = owned
                .hito
                .catalog
                .recording_metadata(&owned.actor, &owned.recording, Default::default())
                .await
                .unwrap_or_else(|_| {
                    panic!("the studio target metadata must be readable before resolution")
                });
            configure_live_candidate(&youtube, title).await;
            let request_id = Uuid::new_v4();
            let resolution = owned
                .hito
                .catalog
                .resolve_source_until(
                    &owned.actor,
                    &owned.recording,
                    ResolveSource { request_id },
                    std::future::pending(),
                )
                .await
                .unwrap_or_else(|_| {
                    panic!("valid live-video evidence must return a typed outcome")
                });
            for path in [
                "v1/recordings/detail",
                "v1/sources",
                "v1/recordings/metadata",
            ] {
                let requests = youtube.requests(path).await;
                assert!(
                    !requests.is_empty()
                        && requests.iter().all(|(_, body)| {
                            body == &json!({"recording_id": "youtube/owned-video"})
                        }),
                    "version policy must be checked after detail, source and metadata hydration"
                );
            }
            assert!(
                resolution.request_id == request_id && resolution.recording_id == owned.recording,
                "version rejection must preserve the request and target identities"
            );
            match resolution.outcome {
                SourceResolutionOutcome::NoSuitableSource => {}
                SourceResolutionOutcome::Ready => panic!(
                    "wrapped live-video evidence must yield NoSuitableSource, received typed Ready"
                ),
                _ => panic!("wrapped live-video evidence must yield NoSuitableSource"),
            }
            assert!(
                resolution.sources.is_empty(),
                "live-video rejection must return no provider sources"
            );
            owned.assert_preserved().await;
            let after = owned
                .hito
                .catalog
                .recording_metadata(&owned.actor, &owned.recording, Default::default())
                .await
                .unwrap_or_else(|_| {
                    panic!("the studio target metadata must remain readable after rejection")
                });
            assert!(
                fixture::encoded(&after) == fixture::encoded(&before),
                "live-video rejection must publish no observation or metadata change"
            );
            assert!(
                owned
                    .hito
                    .catalog
                    .sources(&owned.actor, &owned.recording)
                    .await
                    .unwrap_or_else(|_| panic!("the studio target sources must remain readable"))
                    .is_empty(),
                "live-video rejection must publish no provider source"
            );
            assert!(
                youtube.requests("v1/audio").await.is_empty(),
                "live-video rejection must never request provider audio"
            );
            for path in [
                "v1/search",
                "v1/recordings/detail",
                "v1/sources",
                "v1/recordings/metadata",
                "v1/audio",
            ] {
                assert!(
                    spotify.requests(path).await.is_empty(),
                    "version rejection must never send candidate identifiers to the primary service"
                );
            }
        })
        .catch_unwind(),
    )
    .await;
    spotify.shutdown().await;
    youtube.shutdown().await;
    match result {
        Ok(Ok(())) => {}
        Ok(Err(panic)) => std::panic::resume_unwind(panic),
        Err(_) => {
            panic!("the owned wrapped-live exercise must settle within its test deadline")
        }
    }
}

async fn configure_live_candidate(server: &fixture::ProviderServer, title: &str) {
    let legacy = json!({
        "id": "youtube/owned-video", "title": title,
        "artist": "Owned performer", "duration_ms": 1000, "release": null,
        "disc_number": null, "track_number": null, "revision": "youtube-r1"
    });
    let metadata = json!({
        "recording": legacy, "musical_title": "Owned tune",
        "album_title": "Owned studio album", "source_ids": ["youtube/owned-audio"]
    });
    let sources = json!({"items": [{
        "id": "youtube/owned-audio", "label": "Owned audio", "ready": true
    }]});
    let detail: ProviderRecording = serde_json::from_value(legacy.clone())
        .unwrap_or_else(|_| panic!("the live detail fixture must use the public wire type"));
    let evidence: RecordingMetadata = serde_json::from_value(metadata.clone())
        .unwrap_or_else(|_| panic!("the live metadata fixture must use the public wire type"));
    let page: SourcePage = serde_json::from_value(sources.clone())
        .unwrap_or_else(|_| panic!("the live source fixture must use the public wire type"));
    assert!(
        detail.validate().is_ok() && evidence.validate().is_ok() && page.validate().is_ok(),
        "all live-video protocol evidence must be valid before resolution"
    );
    assert!(
        evidence.recording == detail
            && fixture::encoded(&metadata["recording"]) == fixture::encoded(&legacy)
            && evidence.recording.title == title,
        "metadata must preserve the unchanged full live-video recording response"
    );
    assert!(
        evidence.source_ids == Some(page.items.iter().map(|source| source.id.clone()).collect())
            && page.items.len() == 1
            && page.items[0].ready,
        "the complete live-video source witness must agree with the valid ready source page"
    );
    server
        .reply(
            "v1/search",
            json!({"items": [{
        "id": "youtube/owned-video", "title": title,
        "artist": "channel:Owned channel", "duration_ms": null
    }], "next_cursor": null}),
        )
        .await;
    server.reply("v1/recordings/detail", legacy).await;
    server.reply("v1/recordings/metadata", metadata).await;
    server.reply("v1/sources", sources).await;
}
