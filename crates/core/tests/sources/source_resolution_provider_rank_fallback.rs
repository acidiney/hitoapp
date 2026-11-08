use hito_core::test_support::hang_guard::HANG_CEILING;
#[path = "../fixtures/resolution_musical.rs"]
#[allow(dead_code)]
mod fixture;
use hito_core::test_support::support;

use hito_core::{
    ChooseSource, Credentials, Error, ResolveSource, SourceResolution, SourceResolutionOutcome,
};
use serde_json::{Value, json};
use std::future::pending;
use uuid::Uuid;

struct Ranked {
    id: &'static str,
    title: &'static str,
    artist: &'static str,
    ready: bool,
}

const fn ranked(id: &'static str, title: &'static str) -> Ranked {
    Ranked {
        id,
        title,
        artist: "Owned performer",
        ready: true,
    }
}

const MATCHING: &str = "Owned tune";

struct World {
    spotify: fixture::ProviderServer,
    youtube: fixture::ProviderServer,
    owned: fixture::SavedRecording,
}

impl World {
    async fn start(results: &[Ranked]) -> Self {
        let spotify = fixture::ProviderServer::start().await;
        let youtube = fixture::ProviderServer::start().await;
        let owned = fixture::SavedRecording::prepare(&spotify, &youtube).await;
        configure(&youtube, results).await;
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

    async fn bound_provider_recordings(&self) -> Vec<String> {
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

    async fn finish(self) {
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

async fn configure(server: &fixture::ProviderServer, results: &[Ranked]) {
    let mut summaries = Vec::new();
    for result in results {
        let detail = json!({
            "id": result.id, "title": result.title, "artist": result.artist, "duration_ms": 1000,
            "release": null, "disc_number": null, "track_number": null, "revision": "youtube-r1"
        });
        let source = format!("youtube/{}-audio", result.id);
        let metadata = json!({
            "recording": detail, "musical_title": result.title,
            "album_title": "Owned studio album", "source_ids": [source]
        });
        let sources =
            json!({"items": [{"id": source, "label": "Owned audio", "ready": result.ready}]});
        summaries.push(detail.clone());
        server
            .recording_reply("v1/recordings/detail", result.id, detail)
            .await;
        server
            .recording_reply("v1/recordings/metadata", result.id, metadata)
            .await;
        server
            .recording_reply("v1/sources", result.id, sources)
            .await;
    }
    server
        .reply(
            "v1/search",
            json!({"items": summaries, "next_cursor": null}),
        )
        .await;
}

#[tokio::test]
async fn a_strict_match_wins_over_a_higher_ranked_candidate_that_does_not_match() {
    let world = World::start(&[
        ranked("aaaaaaaaaaa", "Other song"),
        ranked("bbbbbbbbbbb", MATCHING),
    ])
    .await;
    let resolution = world.resolve().await;
    assert!(matches!(resolution.outcome, SourceResolutionOutcome::Ready));
    assert_eq!(match_kinds(&resolution), vec![json!("verified")]);
    assert_eq!(world.bound_provider_recordings().await, vec!["bbbbbbbbbbb"]);
    world.finish().await;
}

#[tokio::test]
async fn without_a_strict_match_the_first_ranked_candidate_is_used_as_a_best_guess() {
    let world = World::start(&[
        ranked("aaaaaaaaaaa", "Other song"),
        ranked("bbbbbbbbbbb", "Another song"),
    ])
    .await;
    let resolution = world.resolve().await;
    assert!(matches!(resolution.outcome, SourceResolutionOutcome::Ready));
    assert_eq!(match_kinds(&resolution), vec![json!("best_guess")]);
    assert_eq!(
        serde_json::to_value(&resolution.sources[0]).unwrap()["provider_label"],
        "Attachment fixture"
    );
    assert_eq!(world.bound_provider_recordings().await, vec!["aaaaaaaaaaa"]);
    for path in [
        "v1/recordings/detail",
        "v1/recordings/metadata",
        "v1/sources",
    ] {
        let hydrated = world.youtube.requests(path).await;
        let mut ids = hydrated
            .iter()
            .map(|(_, body)| body["recording_id"].as_str().unwrap().to_owned())
            .collect::<Vec<_>>();
        ids.sort();
        assert_eq!(
            ids,
            vec!["aaaaaaaaaaa", "bbbbbbbbbbb"],
            "the fallback must reuse candidates the strict pass already hydrated"
        );
    }
    world.owned.assert_preserved().await;
    world.finish().await;
}

#[tokio::test]
async fn an_unready_first_candidate_is_skipped_for_the_next_valid_one() {
    let mut first = ranked("aaaaaaaaaaa", "Other song");
    first.ready = false;
    let world = World::start(&[first, ranked("bbbbbbbbbbb", "Another song")]).await;
    let resolution = world.resolve().await;
    assert!(matches!(resolution.outcome, SourceResolutionOutcome::Ready));
    assert_eq!(match_kinds(&resolution), vec![json!("best_guess")]);
    assert_eq!(world.bound_provider_recordings().await, vec!["bbbbbbbbbbb"]);
    world.finish().await;
}

#[tokio::test]
async fn candidates_without_any_ready_source_still_end_with_no_suitable_source() {
    let mut first = ranked("aaaaaaaaaaa", "Other song");
    first.ready = false;
    let mut second = ranked("bbbbbbbbbbb", "Another song");
    second.ready = false;
    let world = World::start(&[first, second]).await;
    let resolution = world.resolve().await;
    assert!(matches!(
        resolution.outcome,
        SourceResolutionOutcome::NoSuitableSource
    ));
    assert!(resolution.sources.is_empty());
    world.finish().await;
}

#[tokio::test]
async fn a_candidate_the_search_filter_dropped_is_hydrated_once_when_it_is_the_top_result() {
    let mut other = ranked("aaaaaaaaaaa", "Other song");
    other.artist = "Different performer";
    let world = World::start(&[other]).await;
    let resolution = world.resolve().await;
    assert!(matches!(resolution.outcome, SourceResolutionOutcome::Ready));
    assert_eq!(match_kinds(&resolution), vec![json!("best_guess")]);
    assert_eq!(
        world.youtube.requests("v1/recordings/detail").await.len(),
        1
    );
    assert_eq!(world.youtube.requests("v1/search").await.len(), 1);
    world.finish().await;
}

#[tokio::test]
async fn the_best_guess_label_survives_reload_and_is_private_to_the_owner() {
    let world = World::start(&[ranked("aaaaaaaaaaa", "Other song")]).await;
    world.resolve().await;
    let reloaded = world
        .owned
        .hito
        .catalog
        .sources(&world.owned.actor, &world.owned.recording)
        .await
        .unwrap();
    assert_eq!(reloaded.len(), 1);
    assert_eq!(
        serde_json::to_value(&reloaded[0]).unwrap()["match_kind"],
        "best_guess"
    );
    let credentials = || Credentials {
        username: "guest".into(),
        password: "rank fallback guest password".into(),
    };
    world
        .owned
        .hito
        .identity
        .create_user(&world.owned.actor, credentials())
        .await
        .unwrap();
    let session = world
        .owned
        .hito
        .identity
        .login(credentials())
        .await
        .unwrap();
    let guest = world
        .owned
        .hito
        .identity
        .authenticate(&session.token)
        .await
        .unwrap();
    assert!(matches!(
        world
            .owned
            .hito
            .catalog
            .sources(&guest, &world.owned.recording)
            .await,
        Err(Error::NotFound)
    ));
    world.finish().await;
}

#[tokio::test]
async fn choosing_the_source_yourself_replaces_the_best_guess_label_and_stays_protected() {
    let world = World::start(&[ranked("aaaaaaaaaaa", "Other song")]).await;
    let resolution = world.resolve().await;
    let source = Uuid::parse_str(&resolution.sources[0].id).unwrap();
    world
        .owned
        .hito
        .catalog
        .choose_source(
            &world.owned.actor,
            &world.owned.recording,
            ChooseSource {
                request_id: Uuid::new_v4(),
                source_id: source,
            },
        )
        .await
        .unwrap();
    let chosen = world
        .owned
        .hito
        .catalog
        .sources(&world.owned.actor, &world.owned.recording)
        .await
        .unwrap();
    assert_eq!(
        serde_json::to_value(&chosen[0]).unwrap()["match_kind"],
        "verified"
    );
    assert!(matches!(
        world.resolve().await.outcome,
        SourceResolutionOutcome::ProtectedChoice
    ));
    world.finish().await;
}
