use hito_core::test_support::playlist_service as fixture;
use hito_core::test_support::support;

use hito_core::{Action, Credentials, Error, ImportProviderPlaylist};
use serde_json::json;
use uuid::Uuid;

#[tokio::test]
async fn committed_import_replay_survives_provider_outage_and_deletion_but_requires_the_same_live_request()
 {
    let server = fixture::ProviderServer::start().await;
    let (_dir, hito, _setup_actor, _shared, _music) = support::library("playlist-replay").await;
    let session = hito
        .identity
        .login(Credentials {
            username: "admin".into(),
            password: "indexing test password".into(),
        })
        .await
        .unwrap();
    let actor = hito.identity.authenticate(&session.token).await.unwrap();
    let installed = fixture::connect(&hito, &actor, &server, &["playlists"]).await;
    fixture::playlist(&server, "playlist/replay", "snapshot-one", vec![
        json!({"occurrence_id": "retained", "item": {"kind": "recording", "recording": {
            "id": "recording/retained", "title": "Retained recording", "artist": "Fixture artist"
        }}}),
    ]).await;
    let request_id = Uuid::new_v4();
    let request = || ImportProviderPlaylist {
        request_id,
        installation_revision: installed.revision,
        playlist_reference: "https://provider.invalid/replay-selection".into(),
    };
    let committed = hito
        .collection
        .import_provider_playlist(&actor, &installed.id, request())
        .await
        .unwrap();
    let playlist_id = committed.playlist_id;
    let original_receipt = serde_json::to_value(committed).unwrap();
    let original_detail = hito
        .collection
        .playlist(&actor, &playlist_id.to_string())
        .await
        .unwrap();
    assert_eq!(original_detail.entries.len(), 1);
    assert!(Uuid::parse_str(&original_detail.entries[0].entry_id).is_ok());
    support::synchronize(&hito, &actor, vec![Action::PlaylistDelete { playlist_id }]).await;
    let before_replay =
        serde_json::to_value(hito.collection.events(&actor, 0).await.unwrap()).unwrap();
    assert_eq!(before_replay["operations"].as_array().unwrap().len(), 2);
    server.offline().await;
    let detail_requests = server.requests("v1/playlists/detail").await.len();
    let item_requests = server.requests("v1/playlists/items").await.len();

    let replay = hito
        .collection
        .import_provider_playlist(&actor, &installed.id, request())
        .await
        .unwrap();
    assert_eq!(serde_json::to_value(replay).unwrap(), original_receipt);
    assert!(hito.collection.playlists(&actor).await.unwrap().is_empty());
    assert!(matches!(
        hito.collection
            .playlist(&actor, &playlist_id.to_string())
            .await,
        Err(Error::NotFound)
    ));
    let mut changed = request();
    changed.playlist_reference = "https://provider.invalid/different-selection".into();
    assert!(matches!(
        hito.collection
            .import_provider_playlist(&actor, &installed.id, changed)
            .await,
        Err(Error::Conflict(_))
    ));
    let after_replay =
        serde_json::to_value(hito.collection.events(&actor, 0).await.unwrap()).unwrap();
    assert_eq!(after_replay, before_replay);
    assert_eq!(
        server.requests("v1/playlists/detail").await.len(),
        detail_requests
    );
    assert_eq!(
        server.requests("v1/playlists/items").await.len(),
        item_requests
    );
    hito.identity.logout(&session.token).await.unwrap();
    assert!(matches!(
        hito.collection
            .import_provider_playlist(&actor, &installed.id, request())
            .await,
        Err(Error::Unauthorized)
    ));
    assert!(matches!(
        hito.collection.events(&actor, 0).await,
        Err(Error::Unauthorized)
    ));
}
