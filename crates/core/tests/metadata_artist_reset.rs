use hito_core::test_support::support;

use hito_core::{Action, Browse, Error, MetadataHistoryQuery, Operation, Search, SyncBatch};
use lofty::{
    config::WriteOptions,
    prelude::{Accessor, ItemKey, TagExt},
    tag::{Tag, TagType},
};
use support::{library, scan, synchronize, write_wav};
use uuid::Uuid;

#[tokio::test]
async fn performer_reset_restores_current_appearance_credits_without_overwriting_the_title() {
    let (_dir, hito, actor, library, music) = library("credit-reset-host").await;
    for album in ["First", "Second"] {
        let folder = music.join(album);
        std::fs::create_dir(&folder).unwrap();
        let path = folder.join("same.wav");
        write_wav(&path, 0);
        let mut tag = Tag::new(TagType::Id3v2);
        tag.set_title("Inherited title".into());
        tag.set_artist(format!("Observed {album}"));
        tag.set_album(album.into());
        tag.insert_text(ItemKey::AlbumArtist, "Album credit".into());
        tag.save_to_path(path, WriteOptions::default()).unwrap();
    }
    scan(&hito, &actor, &library).await;
    let recording = hito
        .catalog
        .recordings(&actor, Search::default())
        .await
        .unwrap()
        .items
        .remove(0)
        .id;
    let id = Uuid::parse_str(&recording).unwrap();
    synchronize(
        &hito,
        &actor,
        vec![
            Action::RecordingTitleSet {
                recording_id: id,
                title: "My title".into(),
            },
            Action::RecordingArtistSet {
                recording_id: id,
                artist: "My performer".into(),
            },
        ],
    )
    .await;
    let pin = hito
        .catalog
        .recording_metadata(&actor, &recording, MetadataHistoryQuery::default())
        .await
        .unwrap();
    assert_ne!(pin.title.operation_id, pin.artist.operation_id);
    let artist = hito
        .catalog
        .artists(
            &actor,
            Browse {
                q: Some("My performer".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap()
        .items
        .remove(0)
        .id;
    for album in ["First", "Second"] {
        let path = music.join(album).join("same.wav");
        let mut tag = Tag::new(TagType::Id3v2);
        tag.set_title("New inherited title".into());
        tag.set_artist(format!("Retagged {album}"));
        tag.set_album(album.into());
        tag.insert_text(ItemKey::AlbumArtist, "Album credit".into());
        tag.save_to_path(path, WriteOptions::default()).unwrap();
    }
    scan(&hito, &actor, &library).await;
    assert_eq!(
        hito.catalog
            .recordings(&actor, Search::default())
            .await
            .unwrap()
            .items[0]
            .artist,
        "My performer"
    );
    synchronize(
        &hito,
        &actor,
        vec![Action::RecordingArtistInherit { recording_id: id }],
    )
    .await;
    hito.collection
        .apply(
            &actor,
            SyncBatch {
                device_id: "catalog-test".into(),
                operations: vec![Operation {
                    id: Uuid::parse_str(pin.artist.operation_id.as_deref().unwrap()).unwrap(),
                    action: Action::RecordingArtistSet {
                        recording_id: id,
                        artist: "My performer".into(),
                    },
                }],
            },
        )
        .await
        .unwrap();
    let detail = hito
        .catalog
        .recording_metadata(&actor, &recording, MetadataHistoryQuery::default())
        .await
        .unwrap();
    assert!(detail.artist.personal.is_none());
    assert!(detail.artist.value.starts_with("Retagged "));
    assert_eq!(detail.title.value, "My title");
    assert!(matches!(
        hito.catalog
            .artist(&actor, &artist, Browse::default())
            .await,
        Err(Error::NotFound)
    ));
    for release in hito
        .catalog
        .releases(&actor, Browse::default())
        .await
        .unwrap()
        .items
    {
        let detail = hito
            .catalog
            .release(&actor, &release.id, Browse::default())
            .await
            .unwrap();
        assert_eq!(
            detail.tracks.items[0].recording.artist,
            format!("Retagged {}", release.title)
        );
        assert_eq!(detail.tracks.items[0].recording.title, "My title");
    }
    synchronize(
        &hito,
        &actor,
        vec![Action::RecordingArtistSet {
            recording_id: id,
            artist: "My performer".into(),
        }],
    )
    .await;
    hito.identity
        .set_library_access(&actor, &library, &actor.user().id, false)
        .await
        .unwrap();
    assert!(matches!(
        hito.catalog
            .artist(&actor, &artist, Browse::default())
            .await,
        Err(Error::NotFound)
    ));
    hito.identity
        .set_library_access(&actor, &library, &actor.user().id, true)
        .await
        .unwrap();
    assert_eq!(
        hito.catalog
            .artists(
                &actor,
                Browse {
                    q: Some("My performer".into()),
                    ..Default::default()
                }
            )
            .await
            .unwrap()
            .items[0]
            .id,
        artist
    );
}
