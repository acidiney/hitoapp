use crate::resolution_musical_fixture as fixture;
use hito_core::test_support::hang_guard::HANG_CEILING;

use futures_util::FutureExt;
use hito_core::{OpenedAudio, ResolveSource, SourceResolutionOutcome};
use serde_json::json;
use std::panic::AssertUnwindSafe;
use uuid::Uuid;

#[tokio::test]
async fn saved_musical_identity_resolves_wrapped_video_title_with_approved_metadata() {
    let spotify = fixture::ProviderServer::start().await;
    let youtube = fixture::ProviderServer::start().await;
    let result = tokio::time::timeout(
        HANG_CEILING,
        AssertUnwindSafe(async {
            let owned = fixture::SavedRecording::prepare(&spotify, &youtube).await;
            let request_id = Uuid::new_v4();
            let resolution = owned.hito.catalog.resolve_source_until(
                &owned.actor,
                &owned.recording,
                ResolveSource { request_id },
                std::future::pending(),
            ).await.unwrap_or_else(|_| panic!("valid owned resolution must return a typed outcome"));
            assert!(
                resolution.request_id == request_id && resolution.recording_id == owned.recording,
                "automatic resolution must preserve the request and target identities"
            );
            match resolution.outcome {
                SourceResolutionOutcome::Ready => {}
                SourceResolutionOutcome::NoSuitableSource => panic!(
                    "approved musical metadata must yield Ready, received typed NoSuitableSource"
                ),
                _ => panic!("approved musical metadata must yield Ready"),
            }
            assert!(
                resolution.sources.len() == 1
                    && resolution.sources[0].ready
                    && resolution.sources[0].recording_id == owned.recording
                    && resolution.sources[0].provider == "plugin",
                "automatic resolution must publish one ready provider source on the same target"
            );
            let stable = owned.hito.catalog.sources(&owned.actor, &owned.recording).await
                .unwrap_or_else(|_| panic!("the resolved sources must remain readable"));
            assert!(
                fixture::encoded(&stable) == fixture::encoded(&resolution.sources),
                "the published source identity must remain stable"
            );
            owned.assert_preserved().await;
            let metadata = owned.hito.catalog.recording_metadata(&owned.actor, &owned.recording, Default::default()).await
                .unwrap_or_else(|_| panic!("the candidate observation must remain readable"));
            let observed = metadata.observations.items.iter().find(|observation| {
                observation.plugin_installation_id.as_deref() == Some(owned.audio_installation.id.as_str())
            }).unwrap_or_else(|| panic!("the candidate service must retain its own observation"));
            assert!(
                observed.provider == "org.hito.youtube-fixture"
                    && observed.provider_recording_id.as_deref() == Some("youtube/owned-video")
                    && observed.title == "Owned performer - Owned tune (Official Audio)"
                    && observed.artist == "Owned performer" && observed.duration_ms == 1000,
                "candidate provenance must retain that service's unchanged legacy recording"
            );
            for path in ["v1/recordings/metadata", "v1/sources"] {
                assert!(
                    !youtube.requests(path).await.is_empty(),
                    "automatic matching must request approved metadata and matching source witnesses"
                );
            }
            for path in ["v1/recordings/detail", "v1/recordings/metadata", "v1/sources"] {
                assert!(
                    youtube.requests(path).await.iter().all(|(_, body)| {
                        body == &json!({"recording_id": "youtube/owned-video"})
                    }),
                    "candidate requests must use only that service's recording identity"
                );
            }
            assert!(
                youtube.requests("v1/search").await.len() == 1
                    && youtube.requests("v1/audio").await.is_empty(),
                "automatic resolution must discover the source without downloading audio"
            );
            let OpenedAudio::Content(mut audio) = owned.hito.playback.open_audio(
                &owned.actor, &resolution.sources[0].id, Default::default(),
            ).await.unwrap_or_else(|_| panic!("the automatic source must open through the catalog"))
            else { panic!("the automatic source must yield provider audio") };
            assert!(audio.content_type == "audio/wav", "the owned source must preserve its media type");
            let body = audio.body.as_mut()
                .unwrap_or_else(|| panic!("the automatic source must provide an audio body"));
            let mut received = Vec::new();
            while let Some(chunk) = body.chunk().await
                .unwrap_or_else(|_| panic!("the owned provider body must complete")) {
                received.extend_from_slice(&chunk);
            }
            assert!(received == owned.original, "the automatic source must play the exact owned WAV bytes");
            let requests = youtube.requests("v1/audio").await;
            assert!(
                requests.len() == 1 && requests[0].1["source_id"] == "youtube/owned-audio",
                "catalog playback must use the source identified by the metadata witness"
            );
            for path in ["v1/search", "v1/recordings/detail", "v1/recordings/metadata", "v1/sources", "v1/audio"] {
                assert!(
                    spotify.requests(path).await.is_empty(),
                    "automatic matching must never send candidate identifiers to the primary service"
                );
            }
        }).catch_unwind(),
    ).await;
    spotify.shutdown().await;
    youtube.shutdown().await;
    match result {
        Ok(Ok(())) => {}
        Ok(Err(panic)) => std::panic::resume_unwind(panic),
        Err(_) => {
            panic!("the owned musical-evidence exercise must settle within its test deadline")
        }
    }
}

#[tokio::test]
async fn mismatched_musical_source_witness_rejects_ready_audio_without_publication() {
    let spotify = fixture::ProviderServer::start().await;
    let youtube = fixture::ProviderServer::start().await;
    let result = tokio::time::timeout(
        HANG_CEILING,
        AssertUnwindSafe(async {
            let owned = fixture::SavedRecording::prepare(&spotify, &youtube).await;
            let before = owned.hito.catalog.recording_metadata(
                &owned.actor, &owned.recording, Default::default(),
            ).await.unwrap_or_else(|_| panic!("the saved observations must be readable before resolution"));
            let sources = json!({"items": [{
                "id": "youtube/unwitnessed-audio", "label": "Owned audio", "ready": true
            }]});
            let page: hito_plugin_sdk::SourcePage = serde_json::from_value(sources.clone())
                .unwrap_or_else(|_| panic!("the unwitnessed source fixture must use the public wire type"));
            assert!(
                page.validate().is_ok() && page.items.len() == 1
                    && page.items[0].ready && page.items[0].id != "youtube/owned-audio",
                "the sole source mismatch must retain a valid ready source page"
            );
            youtube.reply("v1/sources", sources).await;
            let request_id = Uuid::new_v4();
            let resolution = owned.hito.catalog.resolve_source_until(
                &owned.actor,
                &owned.recording,
                ResolveSource { request_id },
                std::future::pending(),
            ).await.unwrap_or_else(|_| panic!("the witness mismatch must return a typed outcome"));
            assert!(
                resolution.request_id == request_id && resolution.recording_id == owned.recording,
                "rejected resolution must preserve the request and target identities"
            );
            assert!(
                matches!(resolution.outcome, SourceResolutionOutcome::NoSuitableSource)
                    && resolution.sources.is_empty(),
                "a ready source outside the complete metadata witness must yield NoSuitableSource"
            );
            for path in ["v1/recordings/metadata", "v1/sources"] {
                let requests = youtube.requests(path).await;
                assert!(
                    !requests.is_empty() && requests.iter().all(|(_, body)| {
                        body == &json!({"recording_id": "youtube/owned-video"})
                    }),
                    "witness rejection must follow metadata and source exchanges for the same candidate"
                );
            }
            owned.assert_preserved().await;
            let after = owned.hito.catalog.recording_metadata(
                &owned.actor, &owned.recording, Default::default(),
            ).await.unwrap_or_else(|_| panic!("the saved observations must remain readable after rejection"));
            assert!(
                fixture::encoded(&after) == fixture::encoded(&before),
                "witness rejection must publish no candidate observation or metadata change"
            );
            assert!(
                owned.hito.catalog.sources(&owned.actor, &owned.recording).await
                    .unwrap_or_else(|_| panic!("the rejected target sources must remain readable"))
                    .is_empty(),
                "witness rejection must publish no provider source"
            );
            assert!(
                youtube.requests("v1/audio").await.is_empty(),
                "witness rejection must never request provider audio"
            );
            let calls_before = [
                resolution_request_counts(&spotify).await,
                resolution_request_counts(&youtube).await,
            ];
            assert!(
                calls_before[0].iter().all(|count| *count == 0),
                "witness rejection must never send candidate requests to the primary service"
            );
            let replay = owned.hito.catalog.resolve_source_until(
                &owned.actor,
                &owned.recording,
                ResolveSource { request_id },
                std::future::pending(),
            ).await.unwrap_or_else(|_| panic!("the rejected resolution receipt must remain replayable"));
            assert!(
                fixture::encoded(&replay) == fixture::encoded(&resolution),
                "receipt replay must preserve the rejected resolution identity and outcome"
            );
            let calls_after = [
                resolution_request_counts(&spotify).await,
                resolution_request_counts(&youtube).await,
            ];
            assert!(
                calls_after == calls_before,
                "replaying witness rejection must perform no further provider exchanges"
            );
        }).catch_unwind(),
    ).await;
    spotify.shutdown().await;
    youtube.shutdown().await;
    match result {
        Ok(Ok(())) => {}
        Ok(Err(panic)) => std::panic::resume_unwind(panic),
        Err(_) => {
            panic!("the owned witness-mismatch exercise must settle within its test deadline")
        }
    }
}

async fn resolution_request_counts(server: &fixture::ProviderServer) -> Vec<usize> {
    let mut counts = Vec::new();
    for path in [
        "v1/search",
        "v1/recordings/detail",
        "v1/recordings/metadata",
        "v1/sources",
        "v1/audio",
    ] {
        counts.push(server.requests(path).await.len());
    }
    counts
}
