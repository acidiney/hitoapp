use hito_core::test_support::playlist_service as fixture;
use hito_core::test_support::support;

use hito_core::{
    FollowProviderPlaylist, PlaylistFollowInterval, PlaylistFollowStatus, ProviderSearch,
    RefreshFollowedPlaylist,
};
use serde_json::json;
use std::time::Duration;
use uuid::Uuid;

#[tokio::test]
async fn a_held_background_follow_refresh_does_not_block_the_accounts_interactive_search() {
    let server = fixture::ProviderServer::start().await;
    let (_directory, hito, actor, _shared, _music) = support::library("follow-admission").await;
    let installation = fixture::connect(&hito, &actor, &server, &["playlists", "search"]).await;
    fixture::playlist(&server, "playlist/background", "initial", vec![
        json!({"item": {"kind": "recording", "recording": {"id": "recording/initial", "title": "Initial item", "artist": "Fixture artist"}}}),
    ]).await;
    let followed = hito
        .collection
        .follow_provider_playlist(
            &actor,
            &installation.id,
            FollowProviderPlaylist {
                request_id: Uuid::new_v4(),
                installation_revision: installation.revision,
                playlist_reference: "https://provider.invalid/background-selection".into(),
                interval: PlaylistFollowInterval::Hourly,
            },
        )
        .await
        .unwrap();
    let playlist = followed.playlist_id.to_string();
    fixture::playlist(&server, "playlist/background", "refreshed", vec![
        json!({"item": {"kind": "recording", "recording": {"id": "recording/refreshed", "title": "Refreshed item", "artist": "Fixture artist"}}}),
    ]).await;
    server.reply("v1/search", json!({"items": [{"id": "recording/search", "title": "Interactive result", "artist": "Fixture artist"}], "next_cursor": null})).await;
    let held_page = server.hold("v1/playlists/items").await;
    hito.collection
        .enqueue_follow_refresh(
            &actor,
            &playlist,
            RefreshFollowedPlaylist {
                request_id: Uuid::new_v4(),
            },
        )
        .await
        .unwrap();
    let worker = {
        let collection = hito.collection.clone();
        tokio::spawn(async move {
            collection
                .run_next_follow_refresh_until(std::future::pending())
                .await
        })
    };
    tokio::time::timeout(Duration::from_secs(2), held_page.entered())
        .await
        .expect("The real background worker must reach the held provider page");
    let status = hito
        .collection
        .playlist_follow(&actor, &playlist)
        .await
        .unwrap();
    assert_eq!(status.status, PlaylistFollowStatus::Running);
    assert_eq!(status.snapshot, "initial");

    let search = tokio::time::timeout(
        Duration::from_secs(1),
        hito.catalog.search_provider(
            &actor,
            &installation.id,
            ProviderSearch {
                installation_revision: installation.revision,
                query: "Owned interactive request".into(),
                cursor: None,
                limit: 10,
            },
        ),
    )
    .await;
    let requests_while_background_held = server.requests("v1/search").await.len();
    let retained = hito.collection.playlist(&actor, &playlist).await.unwrap();
    held_page.open();
    assert!(
        tokio::time::timeout(Duration::from_secs(3), worker)
            .await
            .expect("The background worker must finish after its page is released")
            .unwrap()
            .unwrap()
    );

    let page = search
        .expect("Interactive search must finish while the background page is held")
        .expect("Background playlist work must leave interactive provider admission available");
    assert_eq!(page.items.len(), 1);
    assert_eq!(page.items[0].id, "recording/search");
    assert_eq!(
        requests_while_background_held, 1,
        "Interactive search must reach the real provider before releasing the background response"
    );
    assert_eq!(retained.entries[0].title, "Initial item");
    let current = hito
        .collection
        .playlist_follow(&actor, &playlist)
        .await
        .unwrap();
    assert_eq!(current.status, PlaylistFollowStatus::Ready);
    assert_eq!(current.snapshot, "refreshed");
}
