#[allow(dead_code)]
#[path = "provider_metadata_backfill.rs"]
mod backfill;
use super::support;
pub use backfill::Library;

use hito_core::{Action, Error};
use serde_json::{Value, json};
use uuid::Uuid;

pub async fn seed_private_state(f: &Library) -> String {
    let copy = f
        .hito
        .collection
        .playlist(&f.actor, &f.first.playlist_id.to_string())
        .await
        .unwrap();
    let recording = copy.entries[0].recording_id.clone().unwrap();
    f.select_owned_alternate_source(&recording).await;
    let recording_id = Uuid::parse_str(&recording).unwrap();
    super::support::synchronize(
        &f.hito,
        &f.actor,
        vec![
            Action::RecordingTitleSet {
                recording_id,
                title: "Private retained title".into(),
            },
            Action::RecordingArtistSet {
                recording_id,
                artist: "Private retained performer".into(),
            },
            Action::LikeSet {
                recording_id,
                liked: true,
            },
            Action::ProgressSet {
                recording_id,
                position_ms: 4_200,
                speed: 1.25,
            },
            Action::PlaylistRename {
                playlist_id: f.first.playlist_id,
                name: "Private retained copy".into(),
            },
        ],
    )
    .await;
    recording
}

pub async fn private_state(f: &Library, recording: &str) -> Value {
    let first = match f
        .hito
        .collection
        .playlist(&f.actor, &f.first.playlist_id.to_string())
        .await
    {
        Ok(copy) => serde_json::to_value(copy).unwrap(),
        Err(Error::NotFound) => Value::Null,
        Err(_) => panic!("The observer could not read the retained copy state"),
    };
    let catalog = f
        .hito
        .catalog
        .recordings(&f.actor, Default::default())
        .await
        .unwrap();
    let mut all_metadata = Vec::new();
    for item in &catalog.items {
        assert!(
            matches!(
                f.hito.downloads.status(&f.actor, &item.id).await,
                Err(Error::NotFound)
            ),
            "Metadata requests must not create a persistent download"
        );
        all_metadata.push(
            f.hito
                .catalog
                .recording_metadata(&f.actor, &item.id, Default::default())
                .await
                .unwrap(),
        );
    }
    let position = f
        .hito
        .collection
        .saved_position(&f.actor, Uuid::parse_str(recording).unwrap())
        .await
        .unwrap()
        .map(|position| json!({"position_ms": position.position_ms}));
    json!({
        "catalog": catalog,
        "albums": f.hito.catalog.releases(&f.actor, Default::default()).await.unwrap(),
        "artists": f.hito.catalog.artists(&f.actor, Default::default()).await.unwrap(),
        "metadata": unrelated_metadata(&f.hito.catalog.recording_metadata(&f.actor, recording, Default::default()).await.unwrap()),
        "all_metadata": all_metadata.iter().map(unrelated_metadata).collect::<Vec<_>>(),
        "sources": f.hito.catalog.sources(&f.actor, recording).await.unwrap(),
        "source_choice": f.hito.catalog.source_choice(&f.actor, recording).await.unwrap(),
        "activity": f.hito.collection.activity(&f.actor).await.unwrap(),
        "position": position,
        "first": first,
        "second": f.hito.collection.playlist(&f.actor, &f.second.playlist_id.to_string()).await.unwrap(),
        "playlists": f.hito.collection.playlists(&f.actor).await.unwrap(),
        "events": f.hito.collection.events(&f.actor, 0).await.unwrap(),
        "plugins": f.hito.plugins.settings(&f.actor).await.unwrap(),
    })
}

pub async fn request_counts(f: &Library) -> Vec<(&'static str, usize, usize)> {
    let mut counts = Vec::new();
    for path in [
        "v1/manifest",
        "v1/playlists/detail",
        "v1/playlists/items",
        "v1/recordings/detail",
        "v1/recordings/metadata",
        "v1/sources",
        "v1/search",
        "v1/audio",
    ] {
        let origin = f.origin.requests(path).await.len();
        let alternate = f.alternate.requests(path).await.len();
        if path == "v1/audio" {
            assert_eq!(
                (origin, alternate),
                (0, 0),
                "Metadata workflows must never fetch audio"
            );
        }
        counts.push((path, origin, alternate));
    }
    counts
}

fn unrelated_metadata(metadata: &hito_core::RecordingMetadata) -> Value {
    let mut snapshot = support::metadata_snapshot(metadata);
    snapshot.as_object_mut().unwrap().remove("appears_in");
    snapshot
}
