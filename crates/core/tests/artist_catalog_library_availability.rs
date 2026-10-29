#[path = "fixtures/album_complete_library.rs"]
#[allow(dead_code)]
mod library;
use hito_core::test_support::album_edition_service::{self as provider, AlbumProvider};
use hito_core::test_support::support;

use hito_core::{
    ChooseAlbumEdition, ClearAlbumEdition, Credentials, DiscographyLibrary, Error,
    ReadDiscographyLibrary, UnlinkRecordingArtist,
};
use library::Album;
use uuid::Uuid;

const OTHER_GROUP: &str = "group:album/owned?kind=ALBUM";

async fn read(album: &Album, ids: &[&str]) -> hito_core::Result<DiscographyLibrary> {
    album
        .hito
        .catalog
        .discography_library(
            &album.actor,
            &album.first,
            &album.association.id,
            ReadDiscographyLibrary {
                release_group_ids: ids.iter().map(|id| id.to_string()).collect(),
            },
        )
        .await
}

fn choose(album: &Album, edition: &str, expected_revision: i64) -> ChooseAlbumEdition {
    ChooseAlbumEdition {
        request_id: Uuid::new_v4(),
        expected_revision,
        recording_id: album.first.clone(),
        association_id: album.association.id.clone(),
        release_group_id: provider::GROUP_ID.into(),
        edition_id: edition.into(),
    }
}

#[tokio::test]
async fn only_an_exactly_linked_album_marks_a_release_group_as_in_the_library() {
    let mut server = AlbumProvider::start().await;
    let album = Album::prepare(&server).await;
    let requests = server.requests().len();

    let before = read(&album, &[provider::GROUP_ID, OTHER_GROUP])
        .await
        .unwrap();
    assert_eq!(before.association_id, album.association.id);
    assert_eq!(before.groups.len(), 2);
    assert_eq!(before.groups[0].release_group_id, provider::GROUP_ID);
    assert!(
        before.groups.iter().all(|group| group.albums.is_empty()),
        "A matching title or artist never links a catalog release group"
    );

    album
        .hito
        .catalog
        .choose_album_edition(
            &album.actor,
            &album.release_id,
            choose(&album, provider::EDITION_ID, 0),
        )
        .await
        .unwrap();
    let linked = read(&album, &[OTHER_GROUP, provider::GROUP_ID])
        .await
        .unwrap();
    assert_eq!(linked.groups[0].release_group_id, OTHER_GROUP);
    assert!(linked.groups[0].albums.is_empty());
    let [found] = linked.groups[1].albums.as_slice() else {
        panic!("The chosen edition links exactly one library album");
    };
    assert_eq!(found.release_id, album.release_id);
    assert_eq!(found.edition_id, provider::EDITION_ID);
    assert_eq!(found.track_count, 3);
    assert_eq!(
        found.playable_count, 0,
        "Provider-adopted metadata without audio is not playable"
    );
    assert_eq!(
        server.requests().len(),
        requests,
        "Availability reads the library only and never contacts the provider"
    );

    album
        .hito
        .catalog
        .clear_album_edition(
            &album.actor,
            &album.release_id,
            ClearAlbumEdition {
                request_id: Uuid::new_v4(),
                expected_revision: 1,
            },
        )
        .await
        .unwrap();
    assert!(
        read(&album, &[provider::GROUP_ID]).await.unwrap().groups[0]
            .albums
            .is_empty()
    );
    assert!(server.finish().await);
}

#[tokio::test]
async fn invalid_requests_unlinked_artists_and_other_accounts_reveal_nothing() {
    let mut server = AlbumProvider::start().await;
    let album = Album::prepare(&server).await;
    album
        .hito
        .catalog
        .choose_album_edition(
            &album.actor,
            &album.release_id,
            choose(&album, provider::EDITION_ID, 0),
        )
        .await
        .unwrap();
    let many: Vec<String> = (0..51).map(|index| format!("group:{index}")).collect();
    let many: Vec<&str> = many.iter().map(String::as_str).collect();
    let long = "x".repeat(257);
    for ids in [
        Vec::new(),
        many,
        vec![provider::GROUP_ID, provider::GROUP_ID],
        vec![long.as_str()],
        vec![""],
    ] {
        assert!(matches!(read(&album, &ids).await, Err(Error::Invalid(_))));
    }

    let credentials = Credentials {
        username: "stranger".into(),
        password: "availability stranger password".into(),
    };
    album
        .hito
        .identity
        .create_user(
            &album.actor,
            Credentials {
                username: credentials.username.clone(),
                password: credentials.password.clone(),
            },
        )
        .await
        .unwrap();
    let session = album.hito.identity.login(credentials).await.unwrap();
    let stranger = album
        .hito
        .identity
        .authenticate(&session.token)
        .await
        .unwrap();
    assert!(matches!(
        album
            .hito
            .catalog
            .discography_library(
                &stranger,
                &album.first,
                &album.association.id,
                ReadDiscographyLibrary {
                    release_group_ids: vec![provider::GROUP_ID.into()],
                },
            )
            .await,
        Err(Error::NotFound)
    ));

    let revision = album
        .hito
        .catalog
        .recording_artist_association_changes(&album.actor, &album.first)
        .await
        .unwrap()
        .revision;
    album
        .hito
        .catalog
        .unlink_recording_artist(
            &album.actor,
            &album.first,
            UnlinkRecordingArtist {
                request_id: Uuid::new_v4(),
                expected_revision: revision,
                association_id: Uuid::parse_str(&album.association.id).unwrap(),
            },
        )
        .await
        .unwrap();
    assert!(matches!(
        read(&album, &[provider::GROUP_ID]).await,
        Err(Error::NotFound | Error::Conflict(_))
    ));
    assert!(server.finish().await);
}
