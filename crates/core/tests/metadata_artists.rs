use hito_core::test_support::support;

use hito_core::{Action, Browse, Credentials, Error, MetadataHistoryQuery, Search};
use lofty::{
    config::WriteOptions,
    prelude::{Accessor, ItemKey, TagExt},
    tag::{Tag, TagType},
};
use support::{library, scan, synchronize, write_wav};
use uuid::Uuid;

#[tokio::test]
async fn personal_performer_changes_all_appearances_and_only_its_accounts_artist_graph() {
    let (_dir, hito, alice, library, music) = library("artist-edit-host").await;
    for album in ["Original", "Compilation"] {
        let folder = music.join(album);
        std::fs::create_dir(&folder).unwrap();
        let path = folder.join("same.wav");
        write_wav(&path, 0);
        let mut tag = Tag::new(TagType::Id3v2);
        tag.set_title("Performance".into());
        tag.set_artist("Observed performer".into());
        tag.set_album(album.into());
        tag.insert_text(ItemKey::AlbumArtist, "Album credit".into());
        tag.save_to_path(path, WriteOptions::default()).unwrap();
    }
    scan(&hito, &alice, &library).await;
    let bob = hito
        .identity
        .create_user(
            &alice,
            Credentials {
                username: "bob".into(),
                password: "private artist password".into(),
            },
        )
        .await
        .unwrap();
    hito.identity
        .set_library_access(&alice, &library, &bob.id, true)
        .await
        .unwrap();
    let session = hito
        .identity
        .login(Credentials {
            username: "bob".into(),
            password: "private artist password".into(),
        })
        .await
        .unwrap();
    let bob = hito.identity.authenticate(&session.token).await.unwrap();
    let recording = hito
        .catalog
        .recordings(&alice, Search::default())
        .await
        .unwrap()
        .items
        .remove(0)
        .id;
    let id = Uuid::parse_str(&recording).unwrap();
    let playlist = Uuid::new_v4();
    synchronize(
        &hito,
        &alice,
        vec![
            Action::PlaylistCreate {
                playlist_id: playlist,
                name: "Performances".into(),
            },
            Action::EntryInsert {
                playlist_id: playlist,
                entry_id: Uuid::new_v4(),
                recording_id: id,
                position: 0,
            },
            Action::ProgressSet {
                recording_id: id,
                position_ms: 100,
                speed: 1.0,
            },
            Action::RecordingArtistSet {
                recording_id: id,
                artist: "Private performer".into(),
            },
        ],
    )
    .await;
    let records = hito
        .catalog
        .recordings(
            &alice,
            Search {
                q: Some("Private performer".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(records.total, 1);
    assert_eq!(records.items[0].artist, "Private performer");
    assert_eq!(
        hito.catalog
            .recordings(
                &alice,
                Search {
                    q: Some("Observed performer".into()),
                    ..Default::default()
                }
            )
            .await
            .unwrap()
            .total,
        0
    );
    assert_eq!(
        hito.collection
            .playlist(&alice, &playlist.to_string())
            .await
            .unwrap()
            .entries[0]
            .artist,
        "Private performer"
    );
    assert_eq!(
        hito.collection.activity(&alice).await.unwrap().history[0].artist,
        "Private performer"
    );
    let artists = hito
        .catalog
        .artists(
            &alice,
            Browse {
                q: Some("Private performer".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(artists.total, 1);
    assert_eq!(artists.items[0].release_count, 2);
    let private = &artists.items[0].id;
    let detail = hito
        .catalog
        .artist(&alice, private, Browse::default())
        .await
        .unwrap();
    assert_eq!(detail.releases.total, 2);
    for album in detail.releases.items {
        let detail = hito
            .catalog
            .release(&alice, &album.id, Browse::default())
            .await
            .unwrap();
        assert_eq!(detail.tracks.items[0].recording.artist, "Private performer");
    }
    assert_eq!(
        hito.catalog
            .artists(
                &alice,
                Browse {
                    q: Some("Observed performer".into()),
                    ..Default::default()
                }
            )
            .await
            .unwrap()
            .total,
        0
    );
    assert_eq!(
        hito.catalog
            .artists(
                &bob,
                Browse {
                    q: Some("Private performer".into()),
                    ..Default::default()
                }
            )
            .await
            .unwrap()
            .total,
        0
    );
    assert!(matches!(
        hito.catalog.artist(&bob, private, Browse::default()).await,
        Err(Error::NotFound)
    ));
    let inherited = hito
        .catalog
        .recordings(&bob, Search::default())
        .await
        .unwrap();
    assert_eq!(inherited.items[0].artist, "Observed performer");
    assert_eq!(
        hito.catalog
            .artists(
                &bob,
                Browse {
                    q: Some("Observed performer".into()),
                    ..Default::default()
                }
            )
            .await
            .unwrap()
            .items[0]
            .release_count,
        2
    );
    let provenance = hito
        .catalog
        .recording_metadata(&alice, &recording, MetadataHistoryQuery::default())
        .await
        .unwrap();
    assert_eq!(
        provenance.artist.personal.as_deref(),
        Some("Private performer")
    );
    assert_eq!(provenance.artist.inherited, "Observed performer");
    assert_eq!(
        provenance.selected_observation.unwrap().artist,
        "Observed performer"
    );
}
