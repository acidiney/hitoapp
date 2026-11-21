#[path = "../fixtures/album_complete_library.rs"]
#[allow(dead_code)]
mod library;
use hito_core::test_support::album_edition_service::{self as provider, AlbumProvider};
use hito_core::test_support::support;

use hito_core::{
    Action, AddAlbum, AlbumAdditionUndone, AlbumEditionTrackListAvailability, Browse, Error,
    Operation, SyncBatch,
};
use library::Album;
use std::collections::HashSet;
use uuid::Uuid;

fn add(album: &Album, request_id: Uuid, edition: &str) -> AddAlbum {
    let read = album.request(edition);
    AddAlbum {
        request_id,
        recording_id: read.recording_id,
        association_id: read.association_id,
        release_group_id: read.release_group_id,
        edition_id: read.edition_id,
    }
}

async fn owned(album: &Album) -> (HashSet<String>, HashSet<String>) {
    let AlbumEditionTrackListAvailability::Available {
        edition, matches, ..
    } = album.read(provider::EDITION_ID).await.unwrap()
    else {
        panic!("the edition must be readable");
    };
    let audio = edition
        .media
        .iter()
        .flat_map(|medium| &medium.tracks)
        .filter(|track| track.kind == hito_plugin_sdk::CatalogTrackKind::Audio)
        .map(|track| track.id.clone())
        .collect();
    let owned = matches
        .into_iter()
        .map(|matched| matched.recording.id)
        .collect();
    (audio, owned)
}

async fn occurrences_owned(album: &Album) -> HashSet<String> {
    let AlbumEditionTrackListAvailability::Available { matches, .. } =
        album.read(provider::EDITION_ID).await.unwrap()
    else {
        panic!("the edition must be readable");
    };
    matches
        .into_iter()
        .map(|matched| matched.occurrence_id)
        .collect()
}

#[tokio::test]
async fn adding_an_album_brings_every_missing_track_in_at_once_and_a_retry_adds_nothing() {
    let server = AlbumProvider::start().await;
    let album = Album::prepare(&server).await;
    let (audio, before) = owned(&album).await;
    let request = Uuid::new_v4();
    let added = album
        .hito
        .artist_catalog
        .add_album(
            &album.actor,
            &album.release_id,
            add(&album, request, provider::EDITION_ID),
        )
        .await
        .unwrap();
    assert!(added.added > 0);
    assert_eq!(
        occurrences_owned(&album).await,
        audio,
        "every audio track of the edition is in the library now"
    );
    let (_, after) = owned(&album).await;
    assert_eq!(after.len(), before.len() + added.added as usize);

    let page = album
        .hito
        .catalog
        .release(&album.actor, &added.release_id, Browse::default())
        .await
        .unwrap();
    assert_eq!(
        page.tracks.items.len(),
        added.added as usize,
        "the added tracks are filed under the chosen album"
    );

    let again = album
        .hito
        .artist_catalog
        .add_album(
            &album.actor,
            &album.release_id,
            add(&album, request, provider::EDITION_ID),
        )
        .await
        .unwrap();
    assert_eq!(again, added, "a repeated request returns the same addition");
    let fresh = album
        .hito
        .artist_catalog
        .add_album(
            &album.actor,
            &album.release_id,
            add(&album, Uuid::new_v4(), provider::EDITION_ID),
        )
        .await
        .unwrap();
    assert_eq!(fresh.added, 0, "nothing is left to add");
    assert!(matches!(
        album
            .hito
            .artist_catalog
            .add_album(
                &album.actor,
                &album.release_id,
                add(&album, request, "edition:other")
            )
            .await,
        Err(Error::Conflict(_))
    ));
}

#[tokio::test]
async fn undo_removes_what_the_addition_brought_and_keeps_a_track_already_used() {
    let server = AlbumProvider::start().await;
    let album = Album::prepare(&server).await;
    let (_, before) = owned(&album).await;
    let added = album
        .hito
        .artist_catalog
        .add_album(
            &album.actor,
            &album.release_id,
            add(&album, Uuid::new_v4(), provider::EDITION_ID),
        )
        .await
        .unwrap();
    let (_, after) = owned(&album).await;
    let liked = after.difference(&before).next().unwrap().clone();
    album
        .hito
        .collection
        .apply(
            &album.actor,
            SyncBatch {
                device_id: "album-addition-test".into(),
                operations: vec![Operation {
                    id: Uuid::new_v4(),
                    action: Action::LikeSet {
                        recording_id: Uuid::parse_str(&liked).unwrap(),
                        liked: true,
                    },
                }],
            },
        )
        .await
        .unwrap();

    let undone = album
        .hito
        .artist_catalog
        .undo_album_addition(&album.actor, added.id)
        .await
        .unwrap();
    assert_eq!(
        undone,
        AlbumAdditionUndone {
            removed: added.added - 1,
            kept: 1
        }
    );
    let (_, left) = owned(&album).await;
    assert_eq!(
        left,
        before
            .iter()
            .cloned()
            .chain([liked])
            .collect::<HashSet<_>>(),
        "only the liked track stays; tracks the account had before are untouched"
    );
    assert_eq!(
        album
            .hito
            .artist_catalog
            .undo_album_addition(&album.actor, added.id)
            .await
            .unwrap(),
        AlbumAdditionUndone {
            removed: 0,
            kept: 0
        },
        "undoing twice changes nothing"
    );
}

#[tokio::test]
async fn nobody_else_can_undo_an_addition() {
    let server = AlbumProvider::start().await;
    let album = Album::prepare(&server).await;
    let added = album
        .hito
        .artist_catalog
        .add_album(
            &album.actor,
            &album.release_id,
            add(&album, Uuid::new_v4(), provider::EDITION_ID),
        )
        .await
        .unwrap();
    let credentials = || hito_core::Credentials {
        username: "album-addition-peer".into(),
        password: "owned album addition peer password".into(),
    };
    album
        .hito
        .identity
        .create_user(&album.actor, credentials())
        .await
        .unwrap();
    let login = album.hito.identity.login(credentials()).await.unwrap();
    let other = album
        .hito
        .identity
        .authenticate(&login.token)
        .await
        .unwrap();
    assert!(matches!(
        album
            .hito
            .artist_catalog
            .undo_album_addition(&other, added.id)
            .await,
        Err(Error::NotFound)
    ));
}

#[tokio::test]
async fn an_album_added_from_the_artist_page_is_owned_on_the_album_page_too() {
    let server = AlbumProvider::start().await;
    let album = Album::prepare(&server).await;
    let (audio, _) = owned(&album).await;
    let added = album
        .hito
        .artist_catalog
        .add_artist_album(
            &album.actor,
            &album.first,
            &album.association.id,
            hito_core::AddArtistAlbum {
                request_id: Uuid::new_v4(),
                release_group_id: provider::GROUP_ID.into(),
                edition_id: provider::EDITION_ID.into(),
            },
        )
        .await
        .unwrap();
    assert!(added.added > 0);
    assert_eq!(occurrences_owned(&album).await, audio);
}
