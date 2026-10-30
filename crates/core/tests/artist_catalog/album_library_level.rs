#[path = "../fixtures/album_complete_library.rs"]
#[allow(dead_code)]
mod library;
use hito_core::test_support::album_edition_service::{self as provider, AlbumProvider};
use hito_core::test_support::support;

use hito_core::{
    AlbumLibraryLevel, ChooseAlbumEdition, Error, LibraryLevel, ReadAlbumLibraryLevel,
    UpdateMetadataSettings,
};
use library::Album;
use uuid::Uuid;

const TRACK_LIST: &str = "v1/catalog/editions/track-list";

async fn choose(album: &Album, edition: &str) {
    let current = album
        .hito
        .artist_catalog
        .release(&album.actor, &album.release_id, Default::default())
        .await
        .unwrap()
        .saved_edition
        .unwrap()
        .revision;
    album
        .hito
        .artist_catalog
        .choose_album_edition(
            &album.actor,
            &album.release_id,
            ChooseAlbumEdition {
                request_id: Uuid::new_v4(),
                expected_revision: current,
                recording_id: album.first.clone(),
                association_id: album.association.id.clone(),
                release_group_id: provider::GROUP_ID.into(),
                edition_id: edition.into(),
            },
        )
        .await
        .unwrap();
}

async fn level(album: &Album, group: &str) -> hito_core::Result<AlbumLibraryLevel> {
    album
        .hito
        .artist_catalog
        .album_library_level_until(
            &album.actor,
            &album.release_id,
            ReadAlbumLibraryLevel {
                release_group_id: group.into(),
            },
            std::future::pending(),
        )
        .await
}

fn other_name(level: &AlbumLibraryLevel) -> String {
    serde_json::to_string(level).unwrap()
}

#[track_caller]
fn compared(level: AlbumLibraryLevel) -> (LibraryLevel, u32, u32) {
    let AlbumLibraryLevel::Compared {
        level: kind,
        matched_tracks,
        edition_tracks,
    } = &level
    else {
        panic!(
            "the remembered edition must compare, got {}",
            other_name(&level)
        );
    };
    (*kind, *matched_tracks, *edition_tracks)
}

#[tokio::test]
async fn a_remembered_edition_classifies_the_album_by_exact_provider_identity() {
    let mut server = AlbumProvider::start().await;
    let album = Album::prepare(&server).await;
    assert!(matches!(
        level(&album, provider::GROUP_ID).await.unwrap(),
        AlbumLibraryLevel::NeedsChoice
    ));
    assert!(
        server.requests_to(TRACK_LIST).is_empty(),
        "without a remembered edition no edition is fetched"
    );

    choose(&album, provider::EDITION_ID).await;
    assert!(
        server.requests_to(TRACK_LIST).is_empty(),
        "remembering an edition makes no provider request"
    );
    let (kind, matched, total) = compared(level(&album, provider::GROUP_ID).await.unwrap());
    assert!(kind == LibraryLevel::Partly);
    assert_eq!(
        (matched, total),
        (3, 6),
        "pregaps are not counted as tracks"
    );
    assert_eq!(server.requests_to(TRACK_LIST).len(), 1);

    library::adopt(
        &album.hito,
        &album.actor,
        &album.installation,
        provider::BONUS,
    )
    .await;
    let before = album.library().await;
    let (kind, matched, total) = compared(level(&album, provider::GROUP_ID).await.unwrap());
    assert!(kind == LibraryLevel::Partly);
    assert_eq!((matched, total), (4, 6));

    choose(&album, provider::STANDARD_ID).await;
    let (kind, matched, total) = compared(level(&album, provider::GROUP_ID).await.unwrap());
    assert!(kind == LibraryLevel::InLibrary);
    assert_eq!((matched, total), (1, 1));

    album
        .sql(
            "UPDATE provider_items SET retired = 1, revision = revision + 1 WHERE recording_id = ? AND role = 'primary'",
            &[&album.second],
        )
        .await;
    choose(&album, provider::EDITION_ID).await;
    let (kind, matched, total) = compared(level(&album, provider::GROUP_ID).await.unwrap());
    assert!(
        kind == LibraryLevel::Partly,
        "a retired binding no longer matches its occurrence"
    );
    assert_eq!((matched, total), (3 - 1 + 1, 6));

    assert_eq!(server.unexpected(), 0);
    assert!(
        server
            .requests()
            .iter()
            .all(|request| !request.path.contains("source") && !request.path.contains("audio")),
        "a comparison never prepares sources or audio"
    );
    let after = album.library().await;
    assert_eq!(
        after["release"]["tracks"], before["release"]["tracks"],
        "personal titles and the album stay as they were"
    );
    assert!(server.finish().await);
}

#[tokio::test]
async fn another_release_group_or_a_missing_edition_never_reaches_the_provider() {
    let mut server = AlbumProvider::start().await;
    let album = Album::prepare(&server).await;
    choose(&album, provider::EDITION_ID).await;
    assert!(matches!(
        level(&album, "group:somewhere/else").await.unwrap(),
        AlbumLibraryLevel::NeedsChoice
    ));
    for invalid in ["", &"x".repeat(257)] {
        assert!(matches!(
            level(&album, invalid).await,
            Err(Error::Invalid(_))
        ));
    }
    assert!(matches!(
        album
            .hito
            .artist_catalog
            .album_library_level_until(
                &album.actor,
                &Uuid::new_v4().to_string(),
                ReadAlbumLibraryLevel {
                    release_group_id: provider::GROUP_ID.into()
                },
                std::future::pending(),
            )
            .await,
        Err(Error::NotFound)
    ));
    assert!(server.requests_to(TRACK_LIST).is_empty());
    assert!(server.finish().await);
}

#[tokio::test]
async fn an_outage_or_disabled_metadata_is_reported_without_losing_the_choice() {
    let mut server = AlbumProvider::start().await;
    let album = Album::prepare(&server).await;
    choose(&album, provider::EDITION_ID).await;
    server.set_outage(true);
    assert!(matches!(
        level(&album, provider::GROUP_ID).await.unwrap(),
        AlbumLibraryLevel::Unavailable
    ));
    server.set_outage(false);
    assert!(matches!(
        level(&album, provider::GROUP_ID).await.unwrap(),
        AlbumLibraryLevel::Compared { .. }
    ));
    server.set_editions(false);
    assert!(matches!(
        level(&album, provider::GROUP_ID).await.unwrap(),
        AlbumLibraryLevel::Unsupported
    ));
    server.set_editions(true);

    let current = album
        .hito
        .artist_catalog
        .metadata_settings(&album.actor)
        .await
        .unwrap();
    album
        .hito
        .artist_catalog
        .set_metadata_settings(
            &album.actor,
            UpdateMetadataSettings {
                revision: current.revision,
                enabled: false,
                providers: current.providers,
            },
        )
        .await
        .unwrap();
    let requests = server.requests().len();
    assert!(matches!(
        level(&album, provider::GROUP_ID).await.unwrap(),
        AlbumLibraryLevel::NeedsChoice | AlbumLibraryLevel::Disabled
    ));
    assert_eq!(server.requests().len(), requests);
    assert!(server.finish().await);
}

#[tokio::test]
async fn a_cancelled_comparison_stops_before_the_provider_answers() {
    let mut server = AlbumProvider::start().await;
    let album = Album::prepare(&server).await;
    choose(&album, provider::EDITION_ID).await;
    let outcome = album
        .hito
        .artist_catalog
        .album_library_level_until(
            &album.actor,
            &album.release_id,
            ReadAlbumLibraryLevel {
                release_group_id: provider::GROUP_ID.into(),
            },
            std::future::ready(()),
        )
        .await
        .unwrap();
    assert!(matches!(outcome, AlbumLibraryLevel::Cancelled));
    assert!(server.finish().await);
}
