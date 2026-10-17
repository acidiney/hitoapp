#[path = "fixtures/playlist_service.rs"]
mod fixture;
mod support;

use hito_core::{
    Credentials, Error, FollowProviderPlaylist, InstallPlugin, PlaylistFollowInterval,
    RefreshFollowedPlaylist,
};
use uuid::Uuid;

#[tokio::test]
async fn manual_refresh_receipts_are_bounded_while_exact_replay_and_other_accounts_remain_available()
 {
    let server = fixture::ProviderServer::start().await;
    let (_directory, hito, actor, _shared, _music) = support::library("follow-retention").await;
    let installation = fixture::connect(&hito, &actor, &server, &["playlists"]).await;
    fixture::playlist(&server, "playlist/manual", "initial", vec![]).await;
    let followed = hito
        .collection
        .follow_provider_playlist(
            &actor,
            &installation.id,
            FollowProviderPlaylist {
                request_id: Uuid::new_v4(),
                installation_revision: installation.revision,
                playlist_reference: "https://provider.invalid/manual-selection".into(),
                interval: PlaylistFollowInterval::Hourly,
            },
        )
        .await
        .unwrap();
    let playlist = followed.playlist_id.to_string();
    hito.identity
        .create_user(
            &actor,
            Credentials {
                username: "other".into(),
                password: "other retention password".into(),
            },
        )
        .await
        .unwrap();
    let other_session = hito
        .identity
        .login(Credentials {
            username: "other".into(),
            password: "other retention password".into(),
        })
        .await
        .unwrap();
    let other = hito
        .identity
        .authenticate(&other_session.token)
        .await
        .unwrap();
    let service = hito
        .plugins
        .settings(&actor)
        .await
        .unwrap()
        .services
        .into_iter()
        .find(|service| service.id == installation.service_id)
        .unwrap();
    let other_installation = hito
        .plugins
        .install(
            &other,
            InstallPlugin {
                service_id: service.id,
                service_revision: service.revision,
                bearer_token: None,
            },
        )
        .await
        .unwrap();
    let other_followed = hito
        .collection
        .follow_provider_playlist(
            &other,
            &other_installation.id,
            FollowProviderPlaylist {
                request_id: Uuid::new_v4(),
                installation_revision: other_installation.revision,
                playlist_reference: "https://provider.invalid/other-manual-selection".into(),
                interval: PlaylistFollowInterval::Daily,
            },
        )
        .await
        .unwrap();
    let first_request = Uuid::new_v4();
    let first = hito
        .collection
        .enqueue_follow_refresh(
            &actor,
            &playlist,
            RefreshFollowedPlaylist {
                request_id: first_request,
            },
        )
        .await
        .unwrap();
    for _ in 1..256 {
        let coalesced = hito
            .collection
            .enqueue_follow_refresh(
                &actor,
                &playlist,
                RefreshFollowedPlaylist {
                    request_id: Uuid::new_v4(),
                },
            )
            .await
            .unwrap();
        assert_eq!(coalesced.job_id, first.job_id);
        assert_eq!(coalesced.playlist_id, followed.playlist_id);
    }
    let replay = hito
        .collection
        .enqueue_follow_refresh(
            &actor,
            &playlist,
            RefreshFollowedPlaylist {
                request_id: first_request,
            },
        )
        .await
        .unwrap();
    assert_eq!(replay.job_id, first.job_id);
    assert_eq!(replay.request_id, first_request);
    let detail_requests = server.requests("v1/playlists/detail").await.len();
    let item_requests = server.requests("v1/playlists/items").await.len();
    let status_before = serde_json::to_value(
        hito.collection
            .playlist_follow(&actor, &playlist)
            .await
            .unwrap(),
    )
    .unwrap();
    let playlist_before =
        serde_json::to_value(hito.collection.playlist(&actor, &playlist).await.unwrap()).unwrap();
    let feed_before =
        serde_json::to_value(hito.collection.events(&actor, 0).await.unwrap()).unwrap();

    let excess = hito
        .collection
        .enqueue_follow_refresh(
            &actor,
            &playlist,
            RefreshFollowedPlaylist {
                request_id: Uuid::new_v4(),
            },
        )
        .await;
    assert!(
        matches!(excess, Err(Error::Conflict(_))),
        "Distinct manual refresh request IDs must respect the account receipt bound even when all requests coalesce into one queued job"
    );
    assert_eq!(
        serde_json::to_value(
            hito.collection
                .playlist_follow(&actor, &playlist)
                .await
                .unwrap()
        )
        .unwrap(),
        status_before
    );
    assert_eq!(
        serde_json::to_value(hito.collection.playlist(&actor, &playlist).await.unwrap()).unwrap(),
        playlist_before
    );
    assert_eq!(
        serde_json::to_value(hito.collection.events(&actor, 0).await.unwrap()).unwrap(),
        feed_before
    );
    assert_eq!(
        server.requests("v1/playlists/detail").await.len(),
        detail_requests
    );
    assert_eq!(
        server.requests("v1/playlists/items").await.len(),
        item_requests
    );
    let replay = hito
        .collection
        .enqueue_follow_refresh(
            &actor,
            &playlist,
            RefreshFollowedPlaylist {
                request_id: first_request,
            },
        )
        .await
        .unwrap();
    assert_eq!(replay.job_id, first.job_id);

    let other_receipt = hito
        .collection
        .enqueue_follow_refresh(
            &other,
            &other_followed.playlist_id.to_string(),
            RefreshFollowedPlaylist {
                request_id: Uuid::new_v4(),
            },
        )
        .await
        .unwrap();
    assert_eq!(other_receipt.playlist_id, other_followed.playlist_id);
    assert_ne!(other_receipt.job_id, first.job_id);
    assert!(
        hito.collection
            .run_next_follow_refresh_until(std::future::pending())
            .await
            .unwrap()
    );
    assert!(
        hito.collection
            .run_next_follow_refresh_until(std::future::pending())
            .await
            .unwrap()
    );
    assert!(
        !hito
            .collection
            .run_next_follow_refresh_until(std::future::pending())
            .await
            .unwrap(),
        "Coalesced manual receipts must leave only one queued job per followed playlist"
    );
}
