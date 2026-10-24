#[path = "fixtures/album_complete_library.rs"]
#[allow(dead_code)]
mod library;
use hito_core::test_support::album_edition_service::{self as provider, AlbumProvider};
use hito_core::test_support::support;

use hito_core::{
    AlbumEditionChoice, AlbumEditionChoiceState, AlbumEditionTrackListAvailability, Browse,
    ChooseAlbumEdition, ClearAlbumEdition, Credentials, Error, UpdateMetadataSettings,
};
use library::Album;
use uuid::Uuid;

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

async fn saved(album: &Album) -> AlbumEditionChoice {
    album
        .hito
        .catalog
        .release(&album.actor, &album.release_id, Browse::default())
        .await
        .unwrap()
        .saved_edition
        .expect("The first page reports the saved edition")
}

async fn read_saved(album: &Album) -> hito_core::Result<AlbumEditionTrackListAvailability> {
    album
        .hito
        .catalog
        .saved_album_edition_track_list_until(
            &album.actor,
            &album.release_id,
            std::future::pending(),
        )
        .await
}

#[tokio::test]
async fn a_chosen_edition_is_remembered_read_cleared_and_replayed_exactly_once() {
    let mut server = AlbumProvider::start().await;
    let album = Album::prepare(&server).await;
    let fresh = saved(&album).await;
    assert_eq!(fresh.revision, 0);
    assert!(matches!(fresh.state, AlbumEditionChoiceState::Unset));
    assert!(matches!(
        read_saved(&album).await.unwrap(),
        AlbumEditionTrackListAvailability::NeedsChoice
    ));
    let requests = server.requests().len();

    let request = choose(&album, provider::EDITION_ID, 0);
    let receipt = album
        .hito
        .catalog
        .choose_album_edition(&album.actor, &album.release_id, request.clone())
        .await
        .unwrap();
    assert_eq!(receipt.revision, 1);
    assert_eq!(receipt.release_id, album.release_id);
    assert_eq!(
        server.requests().len(),
        requests,
        "Saving a choice makes no provider request"
    );
    let replayed = album
        .hito
        .catalog
        .choose_album_edition(&album.actor, &album.release_id, request.clone())
        .await
        .unwrap();
    assert_eq!(replayed, receipt);
    let mut different = request.clone();
    different.edition_id = provider::STANDARD_ID.into();
    assert!(matches!(
        album
            .hito
            .catalog
            .choose_album_edition(&album.actor, &album.release_id, different)
            .await,
        Err(Error::Conflict(_))
    ));
    assert!(matches!(
        album
            .hito
            .catalog
            .choose_album_edition(
                &album.actor,
                &album.release_id,
                choose(&album, provider::STANDARD_ID, 0)
            )
            .await,
        Err(Error::Conflict(_))
    ));

    let current = saved(&album).await;
    assert_eq!(current.revision, 1);
    let AlbumEditionChoiceState::Ready { edition } = current.state else {
        panic!("A readable choice is ready");
    };
    assert_eq!(edition.edition_id, provider::EDITION_ID);
    assert_eq!(edition.release_group_id, provider::GROUP_ID);
    assert_eq!(edition.recording_id, album.first);
    assert_eq!(edition.association_id, album.association.id);

    let AlbumEditionTrackListAvailability::Available {
        edition, matches, ..
    } = read_saved(&album).await.unwrap()
    else {
        panic!("The saved edition reads without choosing again");
    };
    assert_eq!(edition.id, provider::EDITION_ID);
    assert_eq!(matches.len(), 3);
    assert_eq!(
        server.requests_to("v1/catalog/editions/track-list").len(),
        1
    );

    let changed = album
        .hito
        .catalog
        .choose_album_edition(
            &album.actor,
            &album.release_id,
            choose(&album, provider::STANDARD_ID, 1),
        )
        .await
        .unwrap();
    assert_eq!(changed.revision, 2);

    let clear = ClearAlbumEdition {
        request_id: Uuid::new_v4(),
        expected_revision: 2,
    };
    let cleared = album
        .hito
        .catalog
        .clear_album_edition(&album.actor, &album.release_id, clear.clone())
        .await
        .unwrap();
    assert_eq!(cleared.revision, 3);
    assert!(cleared.edition.is_none());
    assert_eq!(
        album
            .hito
            .catalog
            .clear_album_edition(&album.actor, &album.release_id, clear)
            .await
            .unwrap(),
        cleared
    );
    let after = saved(&album).await;
    assert_eq!(after.revision, 3);
    assert!(matches!(after.state, AlbumEditionChoiceState::Unset));
    assert!(matches!(
        read_saved(&album).await.unwrap(),
        AlbumEditionTrackListAvailability::NeedsChoice
    ));
    assert!(server.finish().await);
}

#[tokio::test]
async fn invalid_choices_are_rejected_before_anything_is_saved() {
    let mut server = AlbumProvider::start().await;
    let album = Album::prepare(&server).await;
    let requests = server.requests().len();
    let mut elsewhere = choose(&album, provider::EDITION_ID, 0);
    elsewhere.recording_id = Uuid::new_v4().to_string();
    let mut mismatched = choose(&album, provider::EDITION_ID, 0);
    mismatched.recording_id = album.second.clone();
    let mut empty = choose(&album, provider::EDITION_ID, 0);
    empty.edition_id.clear();
    let mut long = choose(&album, provider::EDITION_ID, 0);
    long.release_group_id = "x".repeat(257);
    let mut nil = choose(&album, provider::EDITION_ID, 0);
    nil.request_id = Uuid::nil();
    for (request, expected) in [
        (elsewhere, "not found"),
        (mismatched, "conflict"),
        (empty, "invalid"),
        (long, "invalid"),
        (nil, "invalid"),
    ] {
        let error = album
            .hito
            .catalog
            .choose_album_edition(&album.actor, &album.release_id, request)
            .await
            .expect_err("An invalid choice must fail");
        assert_eq!(
            match error {
                Error::NotFound => "not found",
                Error::Invalid(_) => "invalid",
                Error::Conflict(_) => "conflict",
                other => panic!("Unexpected error {}", other.code()),
            },
            expected
        );
    }
    assert!(matches!(
        album
            .hito
            .catalog
            .choose_album_edition(
                &album.actor,
                &Uuid::new_v4().to_string(),
                choose(&album, provider::EDITION_ID, 0)
            )
            .await,
        Err(Error::NotFound)
    ));
    let still = saved(&album).await;
    assert_eq!(still.revision, 0);
    assert!(matches!(still.state, AlbumEditionChoiceState::Unset));
    assert_eq!(server.requests().len(), requests);
    assert!(server.finish().await);
}

#[tokio::test]
async fn a_choice_is_private_to_its_account() {
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
    let credentials = Credentials {
        username: "stranger".into(),
        password: "album stranger password".into(),
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
    let requests = server.requests().len();
    assert!(matches!(
        album
            .hito
            .catalog
            .release(&stranger, &album.release_id, Browse::default())
            .await,
        Err(Error::NotFound)
    ));
    assert!(matches!(
        album
            .hito
            .catalog
            .saved_album_edition_track_list_until(
                &stranger,
                &album.release_id,
                std::future::pending()
            )
            .await,
        Err(Error::NotFound)
    ));
    assert!(matches!(
        album
            .hito
            .catalog
            .choose_album_edition(
                &stranger,
                &album.release_id,
                choose(&album, provider::EDITION_ID, 0)
            )
            .await,
        Err(Error::NotFound)
    ));
    assert!(matches!(
        album
            .hito
            .catalog
            .clear_album_edition(
                &stranger,
                &album.release_id,
                ClearAlbumEdition {
                    request_id: Uuid::new_v4(),
                    expected_revision: 1
                }
            )
            .await,
        Err(Error::NotFound)
    ));
    assert_eq!(server.requests().len(), requests);
    assert_eq!(
        saved(&album).await.revision,
        1,
        "The owner's choice is untouched"
    );
    assert!(server.finish().await);
}

#[tokio::test]
async fn a_link_that_can_no_longer_be_read_asks_for_a_new_choice_without_provider_requests() {
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
    let settings = album
        .hito
        .catalog
        .metadata_settings(&album.actor)
        .await
        .unwrap();
    let disabled = album
        .hito
        .catalog
        .set_metadata_settings(
            &album.actor,
            UpdateMetadataSettings {
                revision: settings.revision,
                enabled: false,
                providers: settings.providers.clone(),
            },
        )
        .await
        .unwrap();
    let requests = server.requests().len();
    let stale = saved(&album).await;
    assert_eq!(stale.revision, 1);
    assert!(matches!(stale.state, AlbumEditionChoiceState::NeedsChoice));
    assert!(matches!(
        read_saved(&album).await.unwrap(),
        AlbumEditionTrackListAvailability::NeedsChoice
    ));
    assert!(matches!(
        album
            .hito
            .catalog
            .choose_album_edition(
                &album.actor,
                &album.release_id,
                choose(&album, provider::EDITION_ID, 1)
            )
            .await,
        Err(Error::Conflict(_))
    ));

    album
        .hito
        .catalog
        .set_metadata_settings(
            &album.actor,
            UpdateMetadataSettings {
                revision: disabled.revision,
                enabled: true,
                providers: disabled.providers.clone(),
            },
        )
        .await
        .unwrap();
    assert!(matches!(
        saved(&album).await.state,
        AlbumEditionChoiceState::Ready { .. }
    ));

    album
        .hito
        .plugins
        .remove(
            &album.actor,
            &album.installation.id,
            album.installation.revision,
        )
        .await
        .unwrap();
    let removed = saved(&album).await;
    assert_eq!(removed.revision, 1);
    assert!(matches!(
        removed.state,
        AlbumEditionChoiceState::NeedsChoice
    ));
    assert!(matches!(
        read_saved(&album).await.unwrap(),
        AlbumEditionTrackListAvailability::NeedsChoice
    ));
    assert_eq!(
        server.requests().len(),
        requests,
        "A stale link never reaches a provider"
    );
    assert!(server.finish().await);
}
