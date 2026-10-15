mod support;

use hito_core::{Action, Search};
use lofty::{
    prelude::{Accessor, TagExt},
    tag::{Tag, TagType},
};
use support::{library, scan, synchronize, write_wav};
use uuid::Uuid;

#[tokio::test]
async fn retagging_preserves_recording_source_and_personal_collections() {
    let (_dir, hito, actor, library, music) = library("server").await;
    let path = music.join("original.wav");
    write_wav(&path, 0);
    scan(&hito, &actor, &library).await;
    let original = hito
        .catalog
        .recordings(&actor, Search::default())
        .await
        .unwrap()
        .items
        .remove(0);
    let source = hito
        .catalog
        .sources(&actor, &original.id)
        .await
        .unwrap()
        .remove(0)
        .id;
    let recording_id = Uuid::parse_str(&original.id).unwrap();
    let playlist_id = Uuid::new_v4();
    let entry_id = Uuid::new_v4();
    synchronize(
        &hito,
        &actor,
        vec![
            Action::PlaylistCreate {
                playlist_id,
                name: "Keep my recording".into(),
            },
            Action::EntryInsert {
                playlist_id,
                entry_id,
                recording_id,
                position: 0,
            },
            Action::LikeSet {
                recording_id,
                liked: true,
            },
            Action::ProgressSet {
                recording_id,
                position_ms: 500,
                speed: 1.25,
            },
        ],
    )
    .await;

    let mut tag = Tag::new(TagType::Id3v2);
    tag.set_title("Corrected title".into());
    tag.set_artist("Corrected artist".into());
    tag.set_album("Corrected album".into());
    tag.save_to_path(&path, Default::default()).unwrap();
    scan(&hito, &actor, &library).await;

    let recordings = hito
        .catalog
        .recordings(&actor, Search::default())
        .await
        .unwrap();
    assert_eq!(recordings.total, 1);
    let current = &recordings.items[0];
    assert_eq!(current.id, original.id);
    assert_eq!(current.title, "Corrected title");
    assert_eq!(current.artist, "Corrected artist");
    assert_eq!(current.album.as_deref(), Some("Corrected album"));
    assert!(current.available && current.liked);
    let sources = hito.catalog.sources(&actor, &current.id).await.unwrap();
    assert_eq!(sources.len(), 1);
    assert_eq!(sources[0].id, source);
    assert!(sources[0].ready);
    assert_eq!(
        support::audio_bytes(hito.catalog.audio_file(&actor, &source).await.unwrap()),
        std::fs::read(&path).unwrap()
    );
    let playlist = hito
        .collection
        .playlist(&actor, &playlist_id.to_string())
        .await
        .unwrap();
    assert_eq!(playlist.entries.len(), 1);
    assert_eq!(playlist.entries[0].entry_id, entry_id.to_string());
    assert_eq!(playlist.entries[0].recording_id, original.id);
    assert!(playlist.entries[0].available && playlist.entries[0].liked);
    let history = hito.collection.activity(&actor).await.unwrap().history;
    assert_eq!(history.len(), 1);
    assert_eq!(history[0].id, original.id);
    assert_eq!(history[0].position_ms, 500);
    assert_eq!(history[0].speed, 1.25);
    let found = hito
        .catalog
        .recordings(
            &actor,
            Search {
                q: Some("Corrected".into()),
                ..Search::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(found.total, 1);
}

#[tokio::test]
async fn corrupt_replacement_is_unavailable_and_restoring_verified_bytes_keeps_its_identity() {
    let (_dir, hito, actor, library, music) = library("server").await;
    let path = music.join("original.wav");
    let bytes = write_wav(&path, 0);
    scan(&hito, &actor, &library).await;
    let original = hito
        .catalog
        .recordings(&actor, Search::default())
        .await
        .unwrap()
        .items
        .remove(0);
    let source = hito
        .catalog
        .sources(&actor, &original.id)
        .await
        .unwrap()
        .remove(0)
        .id;
    synchronize(
        &hito,
        &actor,
        vec![Action::LikeSet {
            recording_id: Uuid::parse_str(&original.id).unwrap(),
            liked: true,
        }],
    )
    .await;
    std::fs::write(&path, b"Unreadable replacement").unwrap();
    scan(&hito, &actor, &library).await;
    let current = hito
        .catalog
        .recordings(&actor, Search::default())
        .await
        .unwrap()
        .items
        .remove(0);
    assert_eq!(current.id, original.id);
    assert!(!current.available && current.liked);
    assert!(matches!(
        hito.catalog.audio_file(&actor, &source).await,
        Err(hito_core::Error::NotFound)
    ));
    std::fs::write(&path, bytes).unwrap();
    scan(&hito, &actor, &library).await;
    let restored = hito
        .catalog
        .recordings(&actor, Search::default())
        .await
        .unwrap();
    assert_eq!(restored.total, 1);
    assert_eq!(restored.items[0].id, original.id);
    assert!(restored.items[0].available && restored.items[0].liked);
    let sources = hito.catalog.sources(&actor, &original.id).await.unwrap();
    assert_eq!(sources.len(), 1);
    assert_eq!(sources[0].id, source);
    assert!(sources[0].ready);
}

#[tokio::test]
async fn changing_sample_rate_is_a_distinct_recording_even_with_identical_pcm_bytes() {
    let (_dir, hito, actor, library, music) = library("server").await;
    let path = music.join("original.wav");
    let mut bytes = write_wav(&path, 0);
    scan(&hito, &actor, &library).await;
    let original = hito
        .catalog
        .recordings(&actor, Search::default())
        .await
        .unwrap()
        .items
        .remove(0)
        .id;
    bytes[24..28].copy_from_slice(&16000u32.to_le_bytes());
    bytes[28..32].copy_from_slice(&32000u32.to_le_bytes());
    std::fs::write(&path, bytes).unwrap();
    scan(&hito, &actor, &library).await;
    let recordings = hito
        .catalog
        .recordings(&actor, Search::default())
        .await
        .unwrap();
    assert_eq!(recordings.total, 2);
    assert!(
        !recordings
            .items
            .iter()
            .find(|r| r.id == original)
            .unwrap()
            .available
    );
    assert!(
        recordings
            .items
            .iter()
            .find(|r| r.id != original)
            .unwrap()
            .available
    );
}
