#[path = "fixtures/playlist_service.rs"]
mod fixture;
mod support;

use hito_core::{Action, Error, FollowProviderPlaylist, PlaylistFollowInterval};
use std::time::Instant;
use uuid::Uuid;

#[tokio::test]
async fn deleting_a_follow_releases_its_capacity_without_resurrecting_a_replayed_receipt() {
    let server = fixture::ProviderServer::start().await;
    let (_directory, hito, actor, _shared, _music) = support::library("follow-deletion").await;
    let installation = fixture::connect(&hito, &actor, &server, &["playlists"]).await;
    fixture::playlist(&server, "playlist/capacity", "initial", vec![]).await;
    let request = |request_id| FollowProviderPlaylist {
        request_id,
        installation_revision: installation.revision,
        playlist_reference: "https://provider.invalid/capacity-selection".into(),
        interval: PlaylistFollowInterval::Hourly,
    };
    let original_request = Uuid::new_v4();
    let original = hito
        .collection
        .follow_provider_playlist(&actor, &installation.id, request(original_request))
        .await
        .unwrap();
    let started = Instant::now();
    for _ in 1..256 {
        hito.collection
            .follow_provider_playlist(&actor, &installation.id, request(Uuid::new_v4()))
            .await
            .unwrap();
    }
    eprintln!(
        "Created 256 followed playlists through the public capability in {:?}",
        started.elapsed()
    );
    let at_capacity = hito.collection.playlists(&actor).await.unwrap();
    assert_eq!(at_capacity.len(), 256);
    assert!(
        at_capacity
            .iter()
            .all(|playlist| playlist.kind == "followed")
    );
    let replacement_request = Uuid::new_v4();
    assert!(
        matches!(
            hito.collection
                .follow_provider_playlist(&actor, &installation.id, request(replacement_request))
                .await,
            Err(Error::Conflict(_))
        ),
        "A 257th active follow must respect the account capacity bound"
    );

    support::synchronize(
        &hito,
        &actor,
        vec![Action::PlaylistDelete {
            playlist_id: original.playlist_id,
        }],
    )
    .await;
    assert!(matches!(
        hito.collection
            .playlist_follow(&actor, &original.playlist_id.to_string())
            .await,
        Err(Error::NotFound)
    ));
    let after_delete = hito.collection.playlists(&actor).await.unwrap();
    assert_eq!(after_delete.len(), 255);
    assert!(
        after_delete
            .iter()
            .all(|playlist| playlist.id != original.playlist_id.to_string())
    );
    let detail_requests = server.requests("v1/playlists/detail").await.len();
    let item_requests = server.requests("v1/playlists/items").await.len();
    let replay = hito
        .collection
        .follow_provider_playlist(&actor, &installation.id, request(original_request))
        .await
        .unwrap();
    assert_eq!(replay.playlist_id, original.playlist_id);
    assert_eq!(replay.request_id, original_request);
    assert_eq!(
        server.requests("v1/playlists/detail").await.len(),
        detail_requests
    );
    assert_eq!(
        server.requests("v1/playlists/items").await.len(),
        item_requests
    );
    assert!(matches!(
        hito.collection
            .playlist(&actor, &original.playlist_id.to_string())
            .await,
        Err(Error::NotFound)
    ));

    let replacement = hito
        .collection
        .follow_provider_playlist(&actor, &installation.id, request(replacement_request))
        .await
        .expect("Deleting a followed playlist must release its active-follow capacity");
    assert_ne!(replacement.playlist_id, original.playlist_id);
    let current = hito.collection.playlists(&actor).await.unwrap();
    assert_eq!(current.len(), 256);
    assert!(
        current
            .iter()
            .any(|playlist| playlist.id == replacement.playlist_id.to_string())
    );
    assert!(
        current
            .iter()
            .all(|playlist| playlist.id != original.playlist_id.to_string())
    );
    assert_eq!(
        hito.catalog
            .recordings(&actor, Default::default())
            .await
            .unwrap()
            .total,
        0
    );
}
