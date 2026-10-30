#[path = "../fixtures/provider_metadata_refresh.rs"]
mod fixture;
use hito_core::test_support::support;

use hito_core::{Credentials, Error, RefreshImportedPlaylistMetadata};
use uuid::Uuid;

#[tokio::test]
async fn accepted_metadata_receipts_remain_private_and_require_a_live_session() {
    let f = fixture::Library::start().await;
    let recording = fixture::seed_private_state(&f).await;
    f.publish_verified_album().await;
    let playlist = f.first.playlist_id.to_string();
    let session = f
        .hito
        .identity
        .login(Credentials {
            username: "admin".into(),
            password: "indexing test password".into(),
        })
        .await
        .unwrap();
    let owner = f.hito.identity.authenticate(&session.token).await.unwrap();
    let request_id = Uuid::new_v4();
    let request = || RefreshImportedPlaylistMetadata {
        request_id,
        installation_revision: f.installation.revision,
    };
    let completed = f
        .hito
        .collection
        .refresh_imported_playlist_metadata(&owner, &playlist, request())
        .await
        .unwrap();
    assert_eq!(completed.updated_recordings, 1);
    f.hito
        .identity
        .create_user(
            &f.actor,
            Credentials {
                username: "metadata-other-owner".into(),
                password: "owned metadata other password".into(),
            },
        )
        .await
        .unwrap();
    let other_session = f
        .hito
        .identity
        .login(Credentials {
            username: "metadata-other-owner".into(),
            password: "owned metadata other password".into(),
        })
        .await
        .unwrap();
    let other = f
        .hito
        .identity
        .authenticate(&other_session.token)
        .await
        .unwrap();
    f.origin.offline().await;
    f.alternate.offline().await;
    let before = fixture::private_state(&f, &recording).await;
    let requests = fixture::request_counts(&f).await;
    for attempted_id in [request_id, Uuid::new_v4()] {
        assert!(
            matches!(
                f.hito
                    .collection
                    .refresh_imported_playlist_metadata(
                        &other,
                        &playlist,
                        RefreshImportedPlaylistMetadata {
                            request_id: attempted_id,
                            installation_revision: f.installation.revision
                        }
                    )
                    .await,
                Err(Error::NotFound)
            ),
            "Another account must not refresh the copy or read its receipt"
        );
    }
    assert!(
        f.hito
            .collection
            .playlists(&other)
            .await
            .unwrap()
            .is_empty()
    );
    assert!(
        f.hito
            .collection
            .events(&other, 0)
            .await
            .unwrap()
            .operations
            .is_empty()
    );
    assert_eq!(
        f.hito
            .catalog
            .recordings(&other, Default::default())
            .await
            .unwrap()
            .total,
        0
    );
    f.hito.identity.logout(&session.token).await.unwrap();
    assert!(
        matches!(
            f.hito
                .collection
                .refresh_imported_playlist_metadata(&owner, &playlist, request())
                .await,
            Err(Error::Unauthorized)
        ),
        "A revoked session must not return an existing metadata receipt"
    );
    assert_eq!(fixture::request_counts(&f).await, requests);
    assert!(
        fixture::private_state(&f, &recording).await == before,
        "Account and session denials must preserve the owner's retained private state"
    );
}
