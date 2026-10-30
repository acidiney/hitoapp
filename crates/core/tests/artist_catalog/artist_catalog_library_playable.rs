#[path = "../fixtures/artist_catalog_profiles_library.rs"]
#[allow(dead_code)]
mod library;
use hito_core::test_support::artist_catalog_discography_service as provider;
use hito_core::test_support::support;

use hito_core::{Browse, ChooseAlbumEdition, ReadDiscographyLibrary};
use uuid::Uuid;

#[tokio::test]
async fn a_linked_album_with_owned_audio_reports_playable_tracks_per_exact_group() {
    let library = library::Library::prepare().await;
    let mut server = provider::CatalogServer::start().await;
    let association = library
        .review_artist(&server.url, provider::ARTIST_ID)
        .await;
    let albums = library
        .hito
        .catalog
        .releases(&library.actor, Browse::default())
        .await
        .unwrap();
    assert_eq!(albums.total, 1);
    let release = albums.items[0].id.clone();
    let group = "opaque:release-group/ao-vivo?edição=一";
    let request = || ReadDiscographyLibrary {
        release_group_ids: vec![group.into(), format!("{group} ")],
    };
    let read = || async {
        library
            .hito
            .artist_catalog
            .discography_library(
                &library.actor,
                &library.recording_id,
                &association.id,
                request(),
            )
            .await
            .unwrap()
    };
    assert!(
        read()
            .await
            .groups
            .iter()
            .all(|found| found.albums.is_empty())
    );
    let requests = server.requests();
    library
        .hito
        .artist_catalog
        .choose_album_edition(
            &library.actor,
            &release,
            ChooseAlbumEdition {
                request_id: Uuid::new_v4(),
                expected_revision: 0,
                recording_id: library.recording_id.clone(),
                association_id: association.id.clone(),
                release_group_id: group.into(),
                edition_id: "opaque:edition/一".into(),
            },
        )
        .await
        .unwrap();
    let linked = read().await;
    let [album] = linked.groups[0].albums.as_slice() else {
        panic!("The exact release group links the chosen album");
    };
    assert_eq!(album.release_id, release);
    assert_eq!((album.track_count, album.playable_count), (2, 2));
    assert!(
        linked.groups[1].albums.is_empty(),
        "A near-identical identity is a different release group"
    );
    assert_eq!(server.requests(), requests);
    assert!(server.finish().await);
}
