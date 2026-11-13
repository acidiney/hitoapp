use crate::resolution_musical_fixture as fixture;
use hito_core::test_support::hang_guard::HANG_CEILING;

use futures_util::FutureExt;
use hito_core::{OpenedAudio, ResolveSource, SourceResolutionOutcome};
use hito_plugin_sdk::{ProviderRecording, RecordingMetadata, SearchPage, SourcePage};
use serde_json::json;
use std::panic::AssertUnwindSafe;
use uuid::Uuid;

const FIRST: &str = "zzzzzzzzzzz";

#[tokio::test]
async fn equally_timed_candidates_preserve_the_providers_first_valid_search_result() {
    let spotify = fixture::ProviderServer::start().await;
    let youtube = fixture::ProviderServer::start().await;
    let result = tokio::time::timeout(
        HANG_CEILING,
        AssertUnwindSafe(async {
            let owned = fixture::SavedRecording::prepare(&spotify, &youtube).await;
            configure_ranked_candidates(&youtube).await;
            let request_id = Uuid::new_v4();
            let resolution = owned.hito.catalog.resolve_source_until(
                &owned.actor, &owned.recording, ResolveSource { request_id },
                std::future::pending(),
            ).await.unwrap_or_else(|_| panic!("the ranked search must return a typed resolution"));
            assert!(
                matches!(resolution.outcome, SourceResolutionOutcome::Ready),
                "the provider's valid first search result must yield Ready within the hydration budget"
            );
            assert!(
                resolution.request_id == request_id && resolution.recording_id == owned.recording
                    && resolution.sources.len() == 1 && resolution.sources[0].ready
                    && resolution.sources[0].provider == "plugin"
                    && resolution.sources[0].recording_id == owned.recording,
                "the ranked candidate must publish one ready source on the unchanged Hito recording"
            );
            let searches = youtube.requests("v1/search").await;
            assert!(
                searches.len() == 1 && searches[0].1 == json!({
                    "query": "Owned performer Owned tune", "cursor": null, "limit": 20
                }),
                "ranked selection must use one bounded search with the inherited musical identity"
            );
            for path in ["v1/recordings/detail", "v1/sources", "v1/recordings/metadata"] {
                let requests = youtube.requests(path).await;
                assert!(
                    requests.len() == 1 && requests[0].1 == json!({"recording_id": FIRST}),
                    "the valid first result must be hydrated before opaque-ID-ordered wrong titles"
                );
            }
            owned.assert_preserved().await;
            let metadata = owned.hito.catalog.recording_metadata(
                &owned.actor, &owned.recording, Default::default(),
            ).await.unwrap_or_else(|_| panic!("ranked selection must retain readable metadata"));
            let added = metadata.observations.items.iter().filter(|observation|
                observation.plugin_installation_id.as_deref() ==
                    Some(owned.audio_installation.id.as_str())
            ).collect::<Vec<_>>();
            assert!(
                added.len() == 1 && added[0].provider == "org.hito.youtube-fixture"
                    && added[0].provider_recording_id.as_deref() == Some(FIRST)
                    && added[0].title == "Owned performer - Owned tune (Official Audio)"
                    && added[0].artist == "Owned performer" && added[0].duration_ms == 1000,
                "the observation must belong to the valid first result, never a wrong-title candidate"
            );
            assert!(youtube.requests("v1/audio").await.is_empty(),
                "automatic selection must not fetch audio before an explicit open");
            let OpenedAudio::Content(mut audio) = owned.hito.playback.open_audio(
                &owned.actor, &resolution.sources[0].id, Default::default(),
            ).await.unwrap_or_else(|_| panic!("the selected first source must open"))
            else { panic!("the selected first source must provide content") };
            assert!(audio.content_type == "audio/wav", "the selected source must retain its media type");
            let body = audio.body.as_mut()
                .unwrap_or_else(|| panic!("the selected source must have an audio body"));
            let mut received = Vec::new();
            while let Some(chunk) = body.chunk().await
                .unwrap_or_else(|_| panic!("the selected source body must complete")) {
                received.extend_from_slice(&chunk);
            }
            assert!(received == owned.original, "the selected first result must deliver the exact owned WAV");
            let audio_requests = youtube.requests("v1/audio").await;
            assert!(
                audio_requests.len() == 1 && audio_requests[0].1["source_id"] == "youtube/owned-audio",
                "playback must use the valid first candidate's complete source witness"
            );
            owned.assert_preserved().await;
            for path in ["v1/search", "v1/recordings/detail", "v1/recordings/metadata", "v1/sources", "v1/audio"] {
                assert!(spotify.requests(path).await.is_empty(),
                    "candidate discovery and playback must not call the primary metadata provider");
            }
        }).catch_unwind(),
    ).await;
    spotify.shutdown().await;
    youtube.shutdown().await;
    match result {
        Ok(Ok(())) => {}
        Ok(Err(panic)) => std::panic::resume_unwind(panic),
        Err(_) => panic!("the ranked-search exercise must settle within its owned deadline"),
    }
}

async fn configure_ranked_candidates(server: &fixture::ProviderServer) {
    let candidates = [
        (
            FIRST,
            "Owned performer - Owned tune (Official Audio)",
            "Owned tune",
            "youtube/owned-audio",
        ),
        (
            "aaaaaaaaaaa",
            "Wrong owned tune A",
            "Wrong owned tune A",
            "youtube/wrong-audio-a",
        ),
        (
            "bbbbbbbbbbb",
            "Wrong owned tune B",
            "Wrong owned tune B",
            "youtube/wrong-audio-b",
        ),
        (
            "ccccccccccc",
            "Wrong owned tune C",
            "Wrong owned tune C",
            "youtube/wrong-audio-c",
        ),
    ];
    let mut summaries = Vec::new();
    for (id, title, musical_title, source) in candidates {
        let detail = json!({
            "id": id, "title": title, "artist": "Owned performer", "duration_ms": 1000,
            "release": null, "disc_number": null, "track_number": null, "revision": "youtube-r1"
        });
        let metadata = json!({
            "recording": detail, "musical_title": musical_title,
            "album_title": "Owned studio album", "source_ids": [source]
        });
        let sources = json!({"items": [{"id": source, "label": "Owned audio", "ready": true}]});
        let typed_detail: ProviderRecording = serde_json::from_value(detail.clone())
            .unwrap_or_else(|_| panic!("each candidate detail must use the SDK wire type"));
        let typed_metadata: RecordingMetadata = serde_json::from_value(metadata.clone())
            .unwrap_or_else(|_| {
                panic!("each candidate metadata response must use the SDK wire type")
            });
        let typed_sources: SourcePage = serde_json::from_value(sources.clone())
            .unwrap_or_else(|_| panic!("each candidate source page must use the SDK wire type"));
        assert!(
            typed_detail.validate().is_ok()
                && typed_metadata.validate().is_ok()
                && typed_sources.validate().is_ok()
                && typed_metadata.recording == typed_detail
                && typed_metadata.source_ids == Some(vec![source.to_owned()]),
            "each candidate must have valid ID-consistent detail, metadata and complete source witnesses"
        );
        summaries.push(detail.clone());
        server
            .recording_reply("v1/recordings/detail", id, detail)
            .await;
        server
            .recording_reply("v1/recordings/metadata", id, metadata)
            .await;
        server.recording_reply("v1/sources", id, sources).await;
    }
    let search = json!({"items": summaries, "next_cursor": null});
    let page: SearchPage = serde_json::from_value(search.clone())
        .unwrap_or_else(|_| panic!("the ordered search page must use the SDK wire type"));
    assert!(
        page.validate(20).is_ok()
            && page.items.len() == 4
            && page.items[0].id == FIRST
            && page
                .items
                .iter()
                .all(|item| item.duration_ms == Some(1000) && item.artist == "Owned performer"),
        "the valid first result must share the same admitted duration and artist as all later candidates"
    );
    server.reply("v1/search", search).await;
}
