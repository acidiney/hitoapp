use hito_core::test_support::hang_guard::HANG_CEILING;
#[path = "../fixtures/resolution_musical.rs"]
#[allow(dead_code)]
mod fixture;
use hito_core::test_support::support;

use futures_util::FutureExt;
use hito_core::{ResolveSource, SourceResolutionOutcome};
use serde_json::json;
use std::panic::AssertUnwindSafe;
use uuid::Uuid;

#[tokio::test]
async fn featured_artist_qualifier_resolves_the_plain_titled_candidate() {
    let outcome = resolve_featured_target("Owned song (feat. Guest & Other)", "Owned song").await;
    assert!(
        matches!(outcome, SourceResolutionOutcome::Ready),
        "a saved title that only adds a featured-artist qualifier must resolve the same song"
    );
}

#[tokio::test]
async fn trailing_featured_artist_without_brackets_resolves_the_plain_titled_candidate() {
    let outcome = resolve_featured_target("Owned song ft. Guest", "Owned song").await;
    assert!(
        matches!(outcome, SourceResolutionOutcome::Ready),
        "a trailing ft. credit must not count as a different musical title"
    );
}

#[tokio::test]
async fn featured_artist_qualifier_does_not_hide_a_different_title() {
    let outcome = resolve_featured_target("Owned song (feat. Guest)", "Other song").await;
    assert!(
        matches!(outcome, SourceResolutionOutcome::NoSuitableSource),
        "removing a featured-artist qualifier must still reject a different musical title"
    );
}

async fn resolve_featured_target(target_title: &str, candidate_title: &str) -> SourceResolutionOutcome {
    let spotify = fixture::ProviderServer::start().await;
    let youtube = fixture::ProviderServer::start().await;
    let result = tokio::time::timeout(
        HANG_CEILING,
        AssertUnwindSafe(async {
            let owned = fixture::SavedRecording::prepare_named_target(
                &spotify,
                &youtube,
                target_title,
                "Owned studio album",
            )
            .await;
            configure_candidate(&youtube, candidate_title).await;
            let resolution = owned
                .hito
                .catalog
                .resolve_source_until(
                    &owned.actor,
                    &owned.recording,
                    ResolveSource {
                        request_id: Uuid::new_v4(),
                    },
                    std::future::pending(),
                )
                .await
                .unwrap_or_else(|_| panic!("featured-title resolution must return a typed outcome"));
            owned.assert_preserved().await;
            resolution.outcome
        })
        .catch_unwind(),
    )
    .await;
    spotify.shutdown().await;
    youtube.shutdown().await;
    match result {
        Ok(Ok(outcome)) => outcome,
        Ok(Err(panic)) => std::panic::resume_unwind(panic),
        Err(_) => panic!("the featured-title exercise must settle within its test deadline"),
    }
}

async fn configure_candidate(server: &fixture::ProviderServer, title: &str) {
    let recording = json!({
        "id": "youtube/owned-video", "title": title,
        "artist": "Owned performer", "duration_ms": 1000,
        "release": null, "disc_number": null, "track_number": null, "revision": "youtube-r1"
    });
    let metadata = json!({
        "recording": recording, "musical_title": title,
        "album_title": null, "source_ids": ["youtube/owned-audio"]
    });
    server
        .reply(
            "v1/search",
            json!({"items": [recording.clone()], "next_cursor": null}),
        )
        .await;
    server.reply("v1/recordings/detail", recording).await;
    server.reply("v1/recordings/metadata", metadata).await;
    server
        .reply(
            "v1/sources",
            json!({"items": [{"id": "youtube/owned-audio", "label": "Owned audio", "ready": true}]}),
        )
        .await;
}
