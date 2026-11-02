use hito_core::test_support::collection_artwork as fixture;
use hito_core::test_support::support;
use hito_core::{Credentials, Error, ImportProviderPlaylist};
use uuid::Uuid;

#[tokio::test]
async fn artwork_fetches_are_readable_only_through_their_own_kind_and_account() {
    let provider = fixture::ArtworkServer::start().await;
    let (_directory, hito, owner, _library, _music) = support::library("artwork-kinds").await;
    let installed = provider.connect(&hito, &owner).await;
    let playlist = hito
        .collection
        .import_provider_playlist(
            &owner,
            &installed.id,
            ImportProviderPlaylist {
                request_id: Uuid::new_v4(),
                installation_revision: installed.revision,
                playlist_reference: "https://provider.invalid/owned-artwork".into(),
            },
        )
        .await
        .unwrap()
        .playlist_id
        .to_string();
    let recording = hito
        .catalog
        .recordings(&owner, Default::default())
        .await
        .unwrap()
        .items
        .remove(0);
    let release = recording.release_id.clone().unwrap();
    let recording_fetch = hito
        .catalog
        .enqueue_recording_artwork(&owner, &recording.id)
        .await
        .unwrap()
        .id;
    let release_fetch = hito
        .catalog
        .enqueue_release_artwork(&owner, &release)
        .await
        .unwrap()
        .id;
    let playlist_fetch = hito
        .catalog
        .enqueue_playlist_artwork(&owner, &playlist)
        .await
        .unwrap()
        .id;

    assert_eq!(
        hito.catalog
            .artwork_fetch(&owner, &recording_fetch)
            .await
            .unwrap()
            .recording_id,
        recording.id
    );
    assert_eq!(
        hito.catalog
            .release_artwork_fetch(&owner, &release_fetch)
            .await
            .unwrap()
            .release_id,
        release
    );
    assert_eq!(
        hito.catalog
            .playlist_artwork_fetch(&owner, &playlist_fetch)
            .await
            .unwrap()
            .playlist_id,
        playlist
    );

    assert!(matches!(
        hito.catalog.artwork_fetch(&owner, &release_fetch).await,
        Err(Error::NotFound)
    ));
    assert!(matches!(
        hito.catalog.artwork_fetch(&owner, &playlist_fetch).await,
        Err(Error::NotFound)
    ));
    assert!(matches!(
        hito.catalog
            .release_artwork_fetch(&owner, &recording_fetch)
            .await,
        Err(Error::NotFound)
    ));
    assert!(matches!(
        hito.catalog
            .release_artwork_fetch(&owner, &playlist_fetch)
            .await,
        Err(Error::NotFound)
    ));
    assert!(matches!(
        hito.catalog
            .playlist_artwork_fetch(&owner, &recording_fetch)
            .await,
        Err(Error::NotFound)
    ));
    assert!(matches!(
        hito.catalog
            .playlist_artwork_fetch(&owner, &release_fetch)
            .await,
        Err(Error::NotFound)
    ));

    let credentials = || Credentials {
        username: "listener".into(),
        password: "another listener password".into(),
    };
    hito.identity
        .create_user(&owner, credentials())
        .await
        .unwrap();
    let session = hito.identity.login(credentials()).await.unwrap();
    let other = hito.identity.authenticate(&session.token).await.unwrap();
    assert!(matches!(
        hito.catalog.artwork_fetch(&other, &recording_fetch).await,
        Err(Error::NotFound)
    ));
    assert!(matches!(
        hito.catalog
            .release_artwork_fetch(&other, &release_fetch)
            .await,
        Err(Error::NotFound)
    ));
    assert!(matches!(
        hito.catalog
            .playlist_artwork_fetch(&other, &playlist_fetch)
            .await,
        Err(Error::NotFound)
    ));
}
