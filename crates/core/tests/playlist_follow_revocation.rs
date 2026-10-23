#[path = "fixtures/playlist_read_barrier.rs"]
mod barrier;
use hito_core::test_support::playlist_service as fixture;
use hito_core::test_support::support;

use hito_core::{
    Credentials, Error, FollowProviderPlaylist, PlaylistFollowInterval, PlaylistFollowStatus,
};
use std::time::Duration;
use tracing_subscriber::prelude::*;
use uuid::Uuid;

#[tokio::test]
async fn a_follow_status_read_rechecks_its_session_after_concurrent_logout() {
    let status_row = barrier::HeaderBarrier::matching(
        "SELECT f.interval_seconds,f.snapshot,f.last_refreshed_at,f.next_refresh_at,f.error,",
        "WHERE f.playlist_id = ? AND f.user_id = ? AND f.active = 1",
    );
    tracing::subscriber::set_global_default(
        tracing_subscriber::registry().with(status_row.clone()),
    )
    .unwrap();
    let server = fixture::ProviderServer::start().await;
    let (_directory, hito, _setup_actor, _shared, _music) =
        support::library("follow-revocation").await;
    let credentials = || Credentials {
        username: "admin".into(),
        password: "indexing test password".into(),
    };
    let session = hito.identity.login(credentials()).await.unwrap();
    let actor = hito.identity.authenticate(&session.token).await.unwrap();
    let installation = fixture::connect(&hito, &actor, &server, &["playlists"]).await;
    fixture::playlist(&server, "playlist/revocation", "initial", vec![]).await;
    let followed = hito
        .collection
        .follow_provider_playlist(
            &actor,
            &installation.id,
            FollowProviderPlaylist {
                request_id: Uuid::new_v4(),
                installation_revision: installation.revision,
                playlist_reference: "https://provider.invalid/revocation-selection".into(),
                interval: PlaylistFollowInterval::Hourly,
            },
        )
        .await
        .unwrap();
    let playlist = followed.playlist_id.to_string();
    let before = serde_json::to_value(
        hito.collection
            .playlist_follow(&actor, &playlist)
            .await
            .unwrap(),
    )
    .unwrap();
    let detail_requests = server.requests("v1/playlists/detail").await.len();
    let item_requests = server.requests("v1/playlists/items").await.len();
    let _release_on_drop = status_row.arm();
    let reading = {
        let collection = hito.collection.clone();
        let actor = actor.clone();
        let playlist = playlist.clone();
        tokio::spawn(async move { collection.playlist_follow(&actor, &playlist).await })
    };
    tokio::time::timeout(Duration::from_secs(2), status_row.entered())
        .await
        .expect("The actual status row must reach its SQL completion barrier");
    let logout =
        tokio::time::timeout(Duration::from_secs(3), hito.identity.logout(&session.token)).await;
    status_row.release();
    let read = tokio::time::timeout(Duration::from_secs(2), reading)
        .await
        .expect("The owned status reader must finish after releasing its row")
        .unwrap();
    assert!(
        !status_row.timed_out(),
        "The SQL completion barrier must be released by the test, not its deadline"
    );
    logout
        .expect("Real logout must commit while the old status snapshot is held")
        .unwrap();
    assert!(
        matches!(read, Err(Error::Unauthorized)),
        "A follow status read must reject a session revoked after its initial authorization snapshot"
    );
    let fresh_session = hito.identity.login(credentials()).await.unwrap();
    let fresh_actor = hito
        .identity
        .authenticate(&fresh_session.token)
        .await
        .unwrap();
    let current = hito
        .collection
        .playlist_follow(&fresh_actor, &playlist)
        .await
        .unwrap();
    assert_eq!(current.status, PlaylistFollowStatus::Ready);
    assert_eq!(serde_json::to_value(current).unwrap(), before);
    assert_eq!(
        server.requests("v1/playlists/detail").await.len(),
        detail_requests
    );
    assert_eq!(
        server.requests("v1/playlists/items").await.len(),
        item_requests
    );
}
