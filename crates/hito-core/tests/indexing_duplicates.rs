mod support;

use hito_core::{Error, Search};
use support::{library, scan, write_wav};

#[tokio::test]
async fn replacing_one_duplicate_preserves_the_other_recording_and_source() {
    let (_dir, hito, actor, library, music) = library("server").await;
    let a = music.join("a.wav");
    let b = music.join("b.wav");
    let original_audio = write_wav(&a, 0);
    std::fs::copy(&a, &b).unwrap();
    scan(&hito, &actor, &library).await;

    let mut recordings = hito
        .catalog
        .recordings(&actor, Search::default())
        .await
        .unwrap();
    assert_eq!(recordings.total, 1);
    let original = recordings.items.remove(0);
    let sources = hito.catalog.sources(&actor, &original.id).await.unwrap();
    assert_eq!(sources.len(), 2);
    let original_sources: Vec<_> = sources.into_iter().map(|source| source.id).collect();

    let replacement_audio = write_wav(&a, 1);
    assert_ne!(replacement_audio, original_audio);
    scan(&hito, &actor, &library).await;

    let recordings = hito
        .catalog
        .recordings(&actor, Search::default())
        .await
        .unwrap();
    assert_eq!(recordings.total, 2);
    let unchanged = recordings
        .items
        .iter()
        .find(|recording| recording.id == original.id)
        .unwrap();
    assert!(unchanged.available);
    assert_eq!(unchanged.title, "b");
    assert_eq!(unchanged.artist, original.artist);
    assert_eq!(unchanged.album, original.album);
    assert_eq!(unchanged.duration_ms, original.duration_ms);
    let sources = hito.catalog.sources(&actor, &original.id).await.unwrap();
    assert_eq!(sources.len(), 2);
    let a_source = sources
        .iter()
        .find(|source| !source.ready)
        .unwrap()
        .id
        .clone();
    let b_source = sources
        .iter()
        .find(|source| source.ready)
        .unwrap()
        .id
        .clone();
    assert!(original_sources.contains(&a_source) && original_sources.contains(&b_source));
    assert!(
        !sources
            .iter()
            .find(|source| source.id == a_source)
            .unwrap()
            .ready
    );
    assert!(
        sources
            .iter()
            .find(|source| source.id == b_source)
            .unwrap()
            .ready
    );
    assert!(matches!(
        hito.catalog.audio_file(&actor, &a_source).await,
        Err(Error::NotFound)
    ));
    let path = hito.catalog.audio_file(&actor, &b_source).await.unwrap();
    assert_eq!(support::audio_bytes(path), original_audio);

    let replacement = recordings
        .items
        .iter()
        .find(|recording| recording.id != original.id)
        .unwrap();
    assert!(replacement.available);
    let sources = hito.catalog.sources(&actor, &replacement.id).await.unwrap();
    assert_eq!(sources.len(), 1);
    let replacement_source = &sources[0];
    assert!(replacement_source.ready);
    assert_ne!(replacement_source.id, a_source);
    let path = hito
        .catalog
        .audio_file(&actor, &replacement_source.id)
        .await
        .unwrap();
    assert_eq!(support::audio_bytes(path), replacement_audio);

    scan(&hito, &actor, &library).await;
    assert_eq!(
        hito.catalog
            .recordings(&actor, Search::default())
            .await
            .unwrap()
            .total,
        2
    );
    let sources = hito.catalog.sources(&actor, &replacement.id).await.unwrap();
    assert_eq!(sources.len(), 1);
    assert_eq!(sources[0].id, replacement_source.id);

    std::fs::write(&a, &original_audio).unwrap();
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
            .find(|r| r.id == replacement.id)
            .unwrap()
            .available
    );
    let sources = hito.catalog.sources(&actor, &original.id).await.unwrap();
    let restored = sources
        .iter()
        .find(|source| source.ready && source.id != b_source)
        .unwrap();
    assert_ne!(restored.id, a_source);
    assert_ne!(restored.id, replacement_source.id);
    assert_eq!(
        support::audio_bytes(hito.catalog.audio_file(&actor, &restored.id).await.unwrap()),
        original_audio
    );
    assert!(matches!(
        hito.catalog
            .audio_file(&actor, &replacement_source.id)
            .await,
        Err(Error::NotFound)
    ));
    assert_eq!(std::fs::read(&b).unwrap(), original_audio);
}

#[tokio::test]
async fn a_surviving_duplicate_updates_search_after_its_metadata_owner_is_replaced() {
    use lofty::{
        prelude::{Accessor, TagExt},
        tag::{Tag, TagType},
    };
    let (_dir, hito, actor, library, music) = library("server").await;
    write_wav(&music.join("a.wav"), 0);
    std::fs::copy(music.join("a.wav"), music.join("b.wav")).unwrap();
    scan(&hito, &actor, &library).await;
    let original = hito
        .catalog
        .recordings(&actor, Search::default())
        .await
        .unwrap()
        .items
        .remove(0);
    write_wav(&music.join("a.wav"), 1);
    scan(&hito, &actor, &library).await;
    let mut tag = Tag::new(TagType::Id3v2);
    tag.set_title("Corrected surviving source".into());
    tag.save_to_path(music.join("b.wav"), Default::default())
        .unwrap();
    scan(&hito, &actor, &library).await;
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
    assert_eq!(found.items[0].id, original.id);
    assert!(found.items[0].available);
}

#[tokio::test]
async fn a_surviving_duplicate_updates_search_after_its_metadata_owner_disappears() {
    use lofty::{
        prelude::{Accessor, TagExt},
        tag::{Tag, TagType},
    };
    let (_dir, hito, actor, library, music) = library("server").await;
    write_wav(&music.join("a.wav"), 0);
    std::fs::copy(music.join("a.wav"), music.join("b.wav")).unwrap();
    scan(&hito, &actor, &library).await;
    let original = hito
        .catalog
        .recordings(&actor, Search::default())
        .await
        .unwrap()
        .items
        .remove(0);
    std::fs::remove_file(music.join("a.wav")).unwrap();
    scan(&hito, &actor, &library).await;
    let mut tag = Tag::new(TagType::Id3v2);
    tag.set_title("Corrected surviving source".into());
    tag.save_to_path(music.join("b.wav"), Default::default())
        .unwrap();
    scan(&hito, &actor, &library).await;
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
    assert_eq!(found.items[0].id, original.id);
    assert!(found.items[0].available);
}
