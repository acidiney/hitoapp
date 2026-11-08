use hito_core::test_support::hang_guard::HANG_CEILING;
#[path = "../fixtures/resolution_musical.rs"]
#[allow(dead_code)]
mod fixture;
use hito_core::test_support::support;

use futures_util::FutureExt;
use hito_core::{OpenedAudio, ResolveSource, SourceResolution, SourceResolutionOutcome};
use hito_plugin_sdk::{ProviderRecording, RecordingMetadata, SourcePage};
use serde_json::json;
use std::panic::AssertUnwindSafe;
use uuid::Uuid;

#[tokio::test]
async fn studio_song_named_live_cannot_resolve_a_live_album_recording() {
    exercise_canonical_live("Live Sessions", Expected::Rejected).await;
}

#[tokio::test]
async fn studio_song_named_live_with_wrapped_video_title_plays_owned_audio() {
    exercise_canonical_live("Owned studio album", Expected::Ready).await;
}

#[derive(Clone, Copy)]
enum Expected {
    Rejected,
    Ready,
}

async fn exercise_canonical_live(target_album: &str, expected: Expected) {
    let spotify = fixture::ProviderServer::start().await;
    let youtube = fixture::ProviderServer::start().await;
    let result = tokio::time::timeout(
        HANG_CEILING,
        AssertUnwindSafe(async {
            let owned = fixture::SavedRecording::prepare_named_target(
                &spotify, &youtube, "Live", target_album,
            ).await;
            configure_studio_candidate(&youtube).await;
            let request_id = Uuid::new_v4();
            let resolution = owned.hito.catalog.resolve_source_until(
                &owned.actor,
                &owned.recording,
                ResolveSource { request_id },
                std::future::pending(),
            ).await.unwrap_or_else(|_| panic!("valid canonical-title evidence must return a typed outcome"));
            for path in ["v1/recordings/detail", "v1/sources", "v1/recordings/metadata"] {
                let requests = youtube.requests(path).await;
                assert!(
                    !requests.is_empty() && requests.iter().all(|(_, body)| {
                        body == &json!({"recording_id": "youtube/owned-video"})
                    }),
                    "canonical-title policy must be checked after complete candidate hydration"
                );
            }
            assert!(
                resolution.request_id == request_id && resolution.recording_id == owned.recording,
                "canonical-title resolution must preserve the request and target identities"
            );
            match (expected, resolution.outcome) {
                (Expected::Rejected, SourceResolutionOutcome::Ready)
                    | (Expected::Ready, SourceResolutionOutcome::Ready) => {}
                (_, SourceResolutionOutcome::NoSuitableSource) => panic!(
                    "a technically valid candidate must yield Ready, received typed NoSuitableSource"
                ),
                _ => panic!("canonical-title resolution must return the expected typed outcome"),
            }
            owned.assert_preserved().await;
            let stored = owned.hito.catalog.sources(&owned.actor, &owned.recording).await
                .unwrap_or_else(|_| panic!("the named target sources must remain readable"));
            assert!(
                fixture::encoded(&stored) == fixture::encoded(&resolution.sources),
                "canonical-title resolution must return the stable published source list"
            );
            assert!(
                youtube.requests("v1/audio").await.is_empty(),
                "canonical-title resolution must not request audio before playback"
            );
            for path in ["v1/search", "v1/recordings/detail", "v1/sources", "v1/recordings/metadata", "v1/audio"] {
                assert!(
                    spotify.requests(path).await.is_empty(),
                    "canonical-title resolution must never send candidate identities to the primary service"
                );
            }
            match expected {
                Expected::Rejected => {
                    assert!(
                        stored.len() == 1
                            && serde_json::to_value(&stored[0]).unwrap()["match_kind"] == "best_guess",
                        "the live-album target must only receive the studio source as a best guess"
                    );
                }
                Expected::Ready => assert_studio_playback(&owned, &youtube, &resolution).await,
            }
        }).catch_unwind(),
    ).await;
    spotify.shutdown().await;
    youtube.shutdown().await;
    match result {
        Ok(Ok(())) => {}
        Ok(Err(panic)) => std::panic::resume_unwind(panic),
        Err(_) => panic!("the owned canonical-live exercise must settle within its test deadline"),
    }
}

async fn assert_studio_playback(
    owned: &fixture::SavedRecording,
    server: &fixture::ProviderServer,
    resolution: &SourceResolution,
) {
    assert!(
        serde_json::to_value(&resolution.sources[0]).unwrap()["match_kind"] == "verified",
        "the exact studio match must be verified"
    );
    assert!(
        resolution.sources.len() == 1
            && resolution.sources[0].ready
            && resolution.sources[0].recording_id == owned.recording
            && resolution.sources[0].provider == "plugin",
        "the studio target must retain one ready provider source on its original identity"
    );
    let metadata = owned
        .hito
        .catalog
        .recording_metadata(&owned.actor, &owned.recording, Default::default())
        .await
        .unwrap_or_else(|_| panic!("the studio candidate observation must remain readable"));
    let observed = metadata
        .observations
        .items
        .iter()
        .find(|observation| {
            observation.plugin_installation_id.as_deref()
                == Some(owned.audio_installation.id.as_str())
        })
        .unwrap_or_else(|| panic!("the studio provider must retain its own observation"));
    assert!(
        observed.provider == "org.hito.youtube-fixture"
            && observed.provider_recording_id.as_deref() == Some("youtube/owned-video")
            && observed.title == "Owned performer - Live"
            && observed.artist == "Owned performer"
            && observed.duration_ms == 1000
            && observed.album.as_deref() == Some("Owned studio album"),
        "studio playback must preserve the provider's unchanged full video title and provenance"
    );
    let OpenedAudio::Content(mut audio) = owned
        .hito
        .playback
        .open_audio(&owned.actor, &resolution.sources[0].id, Default::default())
        .await
        .unwrap_or_else(|_| panic!("the studio source must open through the public catalog"))
    else {
        panic!("the studio source must yield owned provider audio")
    };
    assert!(
        audio.content_type == "audio/wav",
        "studio playback must retain the owned media type"
    );
    let body = audio
        .body
        .as_mut()
        .unwrap_or_else(|| panic!("the studio source must provide its owned audio body"));
    let mut received = Vec::new();
    while let Some(chunk) = body
        .chunk()
        .await
        .unwrap_or_else(|_| panic!("the owned studio audio body must complete"))
    {
        received.extend_from_slice(&chunk);
    }
    assert!(
        received == owned.original,
        "the studio target must play the exact owned WAV bytes"
    );
    let requests = server.requests("v1/audio").await;
    assert!(
        requests.len() == 1 && requests[0].1["source_id"] == "youtube/owned-audio",
        "studio playback must use the complete metadata source witness"
    );
}

async fn configure_studio_candidate(server: &fixture::ProviderServer) {
    let legacy = json!({
        "id": "youtube/owned-video", "title": "Owned performer - Live",
        "artist": "Owned performer", "duration_ms": 1000,
        "release": {"id": "youtube/owned-album", "title": "Owned studio album", "artist": "Owned performer"},
        "disc_number": null, "track_number": null, "revision": "youtube-r1"
    });
    let metadata = json!({
        "recording": legacy, "musical_title": "Live",
        "album_title": "Owned studio album", "source_ids": ["youtube/owned-audio"]
    });
    let sources = json!({"items": [{
        "id": "youtube/owned-audio", "label": "Owned audio", "ready": true
    }]});
    let detail: ProviderRecording = serde_json::from_value(legacy.clone())
        .unwrap_or_else(|_| panic!("the studio detail fixture must use the public wire type"));
    let evidence: RecordingMetadata = serde_json::from_value(metadata.clone())
        .unwrap_or_else(|_| panic!("the studio metadata fixture must use the public wire type"));
    let page: SourcePage = serde_json::from_value(sources.clone())
        .unwrap_or_else(|_| panic!("the studio source fixture must use the public wire type"));
    assert!(
        detail.validate().is_ok() && evidence.validate().is_ok() && page.validate().is_ok(),
        "all canonical-title protocol evidence must be valid before resolution"
    );
    assert!(
        evidence.recording == detail
            && fixture::encoded(&metadata["recording"]) == fixture::encoded(&legacy)
            && evidence.recording.title == "Owned performer - Live"
            && evidence.musical_title.as_deref() == Some("Live")
            && evidence.album_title.as_deref() == Some("Owned studio album")
            && detail
                .release
                .as_ref()
                .is_some_and(|release| release.title == "Owned studio album"),
        "the canonical projection must retain the full video title and explicit studio release context"
    );
    assert!(
        evidence.source_ids == Some(page.items.iter().map(|source| source.id.clone()).collect())
            && page.items.len() == 1
            && page.items[0].ready,
        "the complete studio source witness must match the valid ready source page"
    );
    server
        .reply(
            "v1/search",
            json!({"items": [{
        "id": "youtube/owned-video", "title": "Owned performer - Live",
        "artist": "channel:Owned channel", "duration_ms": null
    }], "next_cursor": null}),
        )
        .await;
    server.reply("v1/recordings/detail", legacy).await;
    server.reply("v1/recordings/metadata", metadata).await;
    server.reply("v1/sources", sources).await;
}
