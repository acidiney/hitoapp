mod support;

use hito_core::{Action, Browse, Credentials, Error, MetadataHistoryQuery, Search};
use lofty::{
    config::WriteOptions,
    prelude::{Accessor, ItemKey, TagExt},
    tag::{Tag, TagType},
};
use support::{library, scan, synchronize, write_wav};
use uuid::Uuid;

#[tokio::test]
async fn personal_album_fields_preserve_release_identity_and_private_artist_membership() {
    let (_dir, hito, alice, library, music) = library("release-edit-host").await;
    let path = music.join("track.wav");
    write_wav(&path, 0);
    let mut tag = Tag::new(TagType::Id3v2);
    tag.set_title("Track".into());
    tag.set_artist("Performer".into());
    tag.set_album("Observed album".into());
    tag.insert_text(ItemKey::AlbumArtist, "Observed album credit".into());
    tag.save_to_path(path, WriteOptions::default()).unwrap();
    scan(&hito, &alice, &library).await;
    let bob = hito
        .identity
        .create_user(
            &alice,
            Credentials {
                username: "bob".into(),
                password: "release private password".into(),
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
            password: "release private password".into(),
        })
        .await
        .unwrap();
    let bob = hito.identity.authenticate(&session.token).await.unwrap();
    let release = hito
        .catalog
        .releases(&alice, Browse::default())
        .await
        .unwrap()
        .items
        .remove(0);
    let id = Uuid::parse_str(&release.id).unwrap();
    synchronize(
        &hito,
        &alice,
        vec![
            Action::ReleaseTitleSet {
                release_id: id,
                title: "Private album".into(),
            },
            Action::ReleaseArtistSet {
                release_id: id,
                artist: "Private album credit".into(),
            },
        ],
    )
    .await;
    let albums = hito
        .catalog
        .releases(
            &alice,
            Browse {
                q: Some("Private album".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(albums.total, 1);
    assert_eq!(albums.items[0].id, release.id);
    assert_eq!(albums.items[0].artist, "Private album credit");
    assert_eq!(
        hito.catalog
            .releases(
                &alice,
                Browse {
                    q: Some("Observed album".into()),
                    ..Default::default()
                }
            )
            .await
            .unwrap()
            .total,
        0
    );
    let artist = &albums.items[0].artist_id;
    assert_eq!(
        hito.catalog
            .artist(&alice, artist, Browse::default())
            .await
            .unwrap()
            .releases
            .items[0]
            .id,
        release.id
    );
    assert_eq!(
        hito.catalog
            .artists(
                &alice,
                Browse {
                    q: Some("Observed album credit".into()),
                    ..Default::default()
                }
            )
            .await
            .unwrap()
            .total,
        0
    );
    assert!(matches!(
        hito.catalog.artist(&bob, artist, Browse::default()).await,
        Err(Error::NotFound)
    ));
    let detail = hito
        .catalog
        .release(&alice, &release.id, Browse::default())
        .await
        .unwrap();
    assert_eq!(
        detail.tracks.items[0].recording.album.as_deref(),
        Some("Private album")
    );
    assert_eq!(detail.tracks.items[0].recording.artist, "Performer");
    assert_eq!(
        hito.catalog
            .recordings(&alice, Search::default())
            .await
            .unwrap()
            .items[0]
            .album
            .as_deref(),
        Some("Private album")
    );
    assert_eq!(
        hito.catalog
            .release(&bob, &release.id, Browse::default())
            .await
            .unwrap()
            .release
            .title,
        "Observed album"
    );
    let provenance = hito
        .catalog
        .release_metadata(&alice, &release.id, MetadataHistoryQuery::default())
        .await
        .unwrap();
    assert_eq!(provenance.title.personal.as_deref(), Some("Private album"));
    assert_eq!(
        provenance.artist.personal.as_deref(),
        Some("Private album credit")
    );
    assert_eq!(provenance.title.inherited, "Observed album");
    assert_eq!(
        provenance.observations.items[0].album.as_deref(),
        Some("Observed album")
    );
    let other = hito
        .catalog
        .release_metadata(&bob, &release.id, MetadataHistoryQuery::default())
        .await
        .unwrap();
    assert!(other.title.personal.is_none() && other.artist.personal.is_none());
    assert!(
        !serde_json::to_string(&other)
            .unwrap()
            .contains("Private album")
    );
}
