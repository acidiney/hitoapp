use hito_core::test_support::hang_guard::HANG_CEILING;
#[path = "../fixtures/resolution_musical.rs"]
#[allow(dead_code)]
mod fixture;
use hito_core::test_support::support;

use hito_core::{ResolveSource, SourceResolution, SourceResolutionOutcome};
use hito_plugin_sdk::{ProviderRecording, RecordingMetadata, SearchPage, SourcePage};
use serde_json::{Value, json};
use std::future::pending;
use uuid::Uuid;

struct Row {
    id: &'static str,
    summary_title: &'static str,
    summary_artist: &'static str,
    summary_duration_ms: Option<u64>,
    detail_artist: &'static str,
    detail_duration_ms: u64,
    musical_title: Option<&'static str>,
}

const fn song(id: &'static str, title: &'static str) -> Row {
    Row {
        id,
        summary_title: title,
        summary_artist: "Unknown artist",
        summary_duration_ms: None,
        detail_artist: "Owned performer",
        detail_duration_ms: 1000,
        musical_title: Some(title),
    }
}

const fn video(id: &'static str, title: &'static str, duration_ms: u64) -> Row {
    Row {
        id,
        summary_title: title,
        summary_artist: "Channel: Owned label",
        summary_duration_ms: Some(duration_ms),
        detail_artist: "Channel: Owned label",
        detail_duration_ms: duration_ms,
        musical_title: None,
    }
}

struct World {
    spotify: fixture::ProviderServer,
    youtube: fixture::ProviderServer,
    owned: fixture::SavedRecording,
}

impl World {
    async fn start(rows: &[Row]) -> Self {
        let spotify = fixture::ProviderServer::start().await;
        let youtube = fixture::ProviderServer::start().await;
        let owned = fixture::SavedRecording::prepare(&spotify, &youtube).await;
        configure(&youtube, rows).await;
        Self {
            spotify,
            youtube,
            owned,
        }
    }

    async fn resolve(&self) -> SourceResolution {
        tokio::time::timeout(
            HANG_CEILING,
            self.owned.hito.catalog.resolve_source_until(
                &self.owned.actor,
                &self.owned.recording,
                ResolveSource {
                    request_id: Uuid::new_v4(),
                },
                pending(),
            ),
        )
        .await
        .expect("the resolution must settle")
        .unwrap_or_else(|_| panic!("the resolution must return a typed outcome"))
    }

    async fn bound(&self) -> Vec<String> {
        self.owned
            .hito
            .catalog
            .recording_metadata(&self.owned.actor, &self.owned.recording, Default::default())
            .await
            .unwrap_or_else(|_| panic!("the recording metadata must stay readable"))
            .observations
            .items
            .into_iter()
            .filter(|observation| {
                observation.plugin_installation_id.as_deref()
                    == Some(self.owned.audio_installation.id.as_str())
            })
            .filter_map(|observation| observation.provider_recording_id)
            .collect()
    }

    async fn hydrated(&self) -> Vec<String> {
        let mut ids = self
            .youtube
            .requests("v1/recordings/detail")
            .await
            .iter()
            .map(|(_, body)| body["recording_id"].as_str().unwrap().to_owned())
            .collect::<Vec<_>>();
        ids.sort();
        ids.dedup();
        ids
    }

    async fn finish(self) {
        self.owned.assert_preserved().await;
        self.spotify.shutdown().await;
        self.youtube.shutdown().await;
    }
}

fn match_kinds(resolution: &SourceResolution) -> Vec<Value> {
    resolution
        .sources
        .iter()
        .map(|source| serde_json::to_value(source).unwrap()["match_kind"].clone())
        .collect()
}

async fn configure(server: &fixture::ProviderServer, rows: &[Row]) {
    let mut summaries = Vec::new();
    for row in rows {
        let summary = json!({
            "id": row.id, "title": row.summary_title, "artist": row.summary_artist,
            "duration_ms": row.summary_duration_ms, "release": null, "disc_number": null,
            "track_number": null, "revision": null
        });
        let detail = json!({
            "id": row.id, "title": row.summary_title, "artist": row.detail_artist,
            "duration_ms": row.detail_duration_ms, "release": null, "disc_number": null,
            "track_number": null, "revision": "youtube-r1"
        });
        let source = format!("youtube/{}-audio", row.id);
        let metadata = json!({
            "recording": detail, "musical_title": row.musical_title,
            "album_title": row.musical_title.map(|_| "Owned studio album"), "source_ids": [source]
        });
        let sources = json!({"items": [{"id": source, "label": "Owned audio", "ready": true}]});
        let typed_summary: ProviderRecording = serde_json::from_value(summary.clone()).unwrap();
        let typed_metadata: RecordingMetadata = serde_json::from_value(metadata.clone()).unwrap();
        let typed_sources: SourcePage = serde_json::from_value(sources.clone()).unwrap();
        assert!(
            typed_summary.validate().is_ok()
                && typed_metadata.validate().is_ok()
                && typed_sources.validate().is_ok(),
            "every row must use valid SDK wire types"
        );
        summaries.push(summary);
        server
            .recording_reply("v1/recordings/detail", row.id, detail)
            .await;
        server
            .recording_reply("v1/recordings/metadata", row.id, metadata)
            .await;
        server.recording_reply("v1/sources", row.id, sources).await;
    }
    let search = json!({"items": summaries, "next_cursor": null});
    let page: SearchPage = serde_json::from_value(search.clone()).unwrap();
    assert!(page.validate(20).is_ok());
    server.reply("v1/search", search).await;
}

#[tokio::test]
async fn a_leading_video_leaves_the_strictly_matching_second_catalog_song_verified() {
    let world = World::start(&[
        video("OwnedVid000", "Owned performer - Owned tune (Live)", 1000),
        song("OwnedSong00", "Owned other tune"),
        song("OwnedSong01", "Owned tune"),
        song("OwnedSong02", "Owned third tune"),
        video("OwnedVid001", "Owned performer - Owned tune (Lyrics)", 1000),
    ])
    .await;
    let resolution = world.resolve().await;
    assert!(matches!(resolution.outcome, SourceResolutionOutcome::Ready));
    assert_eq!(match_kinds(&resolution), vec![json!("verified")]);
    assert_eq!(world.bound().await, vec!["OwnedSong01"]);
    assert_eq!(
        world.hydrated().await,
        vec!["OwnedSong00", "OwnedSong01", "OwnedVid000"],
        "the strict window must stay bounded to the leading video and the first catalog songs"
    );
    world.finish().await;
}

#[tokio::test]
async fn without_a_leading_video_the_third_catalog_song_stays_in_the_strict_window() {
    let world = World::start(&[
        song("OwnedSong00", "Owned other tune"),
        song("OwnedSong01", "Owned third tune"),
        song("OwnedSong02", "Owned tune"),
        video("OwnedVid000", "Owned performer - Owned tune (Lyrics)", 1000),
    ])
    .await;
    let resolution = world.resolve().await;
    assert!(matches!(resolution.outcome, SourceResolutionOutcome::Ready));
    assert_eq!(match_kinds(&resolution), vec![json!("verified")]);
    assert_eq!(world.bound().await, vec!["OwnedSong02"]);
    assert!(
        !world.hydrated().await.contains(&"OwnedVid000".to_owned()),
        "a trailing video must not be hydrated when a catalog song matches"
    );
    world.finish().await;
}

#[tokio::test]
async fn a_leading_video_is_the_best_guess_when_no_catalog_song_matches() {
    let world = World::start(&[
        video("OwnedVid000", "Owned performer - Owned tune", 1000),
        song("OwnedSong00", "Owned other tune"),
        song("OwnedSong01", "Owned third tune"),
        song("OwnedSong02", "Owned fourth tune"),
    ])
    .await;
    let resolution = world.resolve().await;
    assert!(matches!(resolution.outcome, SourceResolutionOutcome::Ready));
    assert_eq!(
        match_kinds(&resolution),
        vec![json!("best_guess")],
        "a video without musical evidence must never be verified"
    );
    assert_eq!(world.bound().await, vec!["OwnedVid000"]);
    world.finish().await;
}

#[tokio::test]
async fn a_leading_video_with_a_different_summary_duration_is_still_the_best_guess() {
    let world = World::start(&[
        video("OwnedVid000", "Owned performer - Owned tune", 1300),
        song("OwnedSong00", "Owned other tune"),
        song("OwnedSong01", "Owned third tune"),
    ])
    .await;
    let resolution = world.resolve().await;
    assert!(matches!(resolution.outcome, SourceResolutionOutcome::Ready));
    assert_eq!(match_kinds(&resolution), vec![json!("best_guess")]);
    assert_eq!(world.bound().await, vec!["OwnedVid000"]);
    world.finish().await;
}
