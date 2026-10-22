use hito_core::test_support::playlist_service as fixture;
use hito_core::test_support::support;

use hito_core::{Error, FollowProviderPlaylist, PlaylistFollowInterval, RefreshFollowedPlaylist};
use sqlx::{Connection, SqliteConnection, sqlite::SqliteConnectOptions};
use uuid::Uuid;

async fn age_receipts(
    database: &std::path::Path,
    account: &str,
    retained: Uuid,
    terminal_job: Uuid,
) {
    let mut connection = SqliteConnection::connect_with(
        &SqliteConnectOptions::new()
            .filename(database)
            .foreign_keys(true),
    )
    .await
    .unwrap();
    let mut tx = connection.begin().await.unwrap();
    let expired = sqlx::query("UPDATE playlist_follow_refresh_receipts SET created_at = unixepoch() - 691200 WHERE user_id = ? AND request_id != ?")
        .bind(account).bind(retained.to_string()).execute(&mut *tx).await.unwrap().rows_affected();
    assert_eq!(expired, 255);
    let aged_job = sqlx::query("UPDATE playlist_follow_jobs SET created_at = unixepoch() - 691200,updated_at = unixepoch() - 691200 WHERE id = ? AND status = 'complete'")
        .bind(terminal_job.to_string()).execute(&mut *tx).await.unwrap().rows_affected();
    assert_eq!(aged_job, 1);
    tx.commit().await.unwrap();
    connection.close().await.unwrap();
}

#[tokio::test]
async fn expired_terminal_refresh_receipts_release_capacity_without_losing_unexpired_or_pending_replays()
 {
    let server = fixture::ProviderServer::start().await;
    let (directory, hito, actor, _shared, _music) = support::library("follow-expiry").await;
    let installation = fixture::connect(&hito, &actor, &server, &["playlists"]).await;
    fixture::playlist(&server, "playlist/expiry", "initial", vec![]).await;
    let follow_request = Uuid::new_v4();
    let follow = || FollowProviderPlaylist {
        request_id: follow_request,
        installation_revision: installation.revision,
        playlist_reference: "https://provider.invalid/expiry-selection".into(),
        interval: PlaylistFollowInterval::Hourly,
    };
    let followed = hito
        .collection
        .follow_provider_playlist(&actor, &installation.id, follow())
        .await
        .unwrap();
    let playlist = followed.playlist_id.to_string();
    let first_request = Uuid::new_v4();
    let terminal = hito
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
    let mut unexpired_request = first_request;
    for _ in 1..255 {
        unexpired_request = Uuid::new_v4();
        let coalesced = hito
            .collection
            .enqueue_follow_refresh(
                &actor,
                &playlist,
                RefreshFollowedPlaylist {
                    request_id: unexpired_request,
                },
            )
            .await
            .unwrap();
        assert_eq!(coalesced.job_id, terminal.job_id);
    }
    assert!(
        hito.collection
            .run_next_follow_refresh_until(std::future::pending())
            .await
            .unwrap()
    );
    let pending_request = Uuid::new_v4();
    let pending = hito
        .collection
        .enqueue_follow_refresh(
            &actor,
            &playlist,
            RefreshFollowedPlaylist {
                request_id: pending_request,
            },
        )
        .await
        .unwrap();
    assert_ne!(pending.job_id, terminal.job_id);
    server.offline().await;
    assert!(
        matches!(
            hito.collection
                .enqueue_follow_refresh(
                    &actor,
                    &playlist,
                    RefreshFollowedPlaylist {
                        request_id: Uuid::new_v4()
                    }
                )
                .await,
            Err(Error::Conflict(_))
        ),
        "Unexpired receipts must first fill the configured account limit"
    );
    let before =
        serde_json::to_value(hito.collection.playlist(&actor, &playlist).await.unwrap()).unwrap();
    let status_before = serde_json::to_value(
        hito.collection
            .playlist_follow(&actor, &playlist)
            .await
            .unwrap(),
    )
    .unwrap();
    let detail_requests = server.requests("v1/playlists/detail").await.len();
    let item_requests = server.requests("v1/playlists/items").await.len();
    age_receipts(
        &directory.path().join("hito.db"),
        &actor.user().id,
        unexpired_request,
        terminal.job_id,
    )
    .await;

    let fresh = hito
        .collection
        .enqueue_follow_refresh(
            &actor,
            &playlist,
            RefreshFollowedPlaylist {
                request_id: Uuid::new_v4(),
            },
        )
        .await
        .expect(
            "Manual receipts older than seven days for terminal jobs must release request capacity",
        );
    assert_eq!(fresh.job_id, pending.job_id);
    let retained = hito
        .collection
        .enqueue_follow_refresh(
            &actor,
            &playlist,
            RefreshFollowedPlaylist {
                request_id: unexpired_request,
            },
        )
        .await
        .unwrap();
    assert_eq!(retained.job_id, terminal.job_id);
    let changed_pending = hito
        .collection
        .enqueue_follow_refresh(
            &actor,
            &Uuid::new_v4().to_string(),
            RefreshFollowedPlaylist {
                request_id: pending_request,
            },
        )
        .await;
    assert!(
        matches!(changed_pending, Err(Error::Conflict(_))),
        "An old receipt for a still-pending job must retain its immutable playlist binding"
    );
    let pending_replay = hito
        .collection
        .enqueue_follow_refresh(
            &actor,
            &playlist,
            RefreshFollowedPlaylist {
                request_id: pending_request,
            },
        )
        .await
        .unwrap();
    assert_eq!(pending_replay.job_id, pending.job_id);
    let permanent = hito
        .collection
        .follow_provider_playlist(&actor, &installation.id, follow())
        .await
        .unwrap();
    assert_eq!(permanent.playlist_id, followed.playlist_id);
    assert_eq!(permanent.request_id, follow_request);
    assert_eq!(
        serde_json::to_value(hito.collection.playlist(&actor, &playlist).await.unwrap()).unwrap(),
        before
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
        server.requests("v1/playlists/detail").await.len(),
        detail_requests
    );
    assert_eq!(
        server.requests("v1/playlists/items").await.len(),
        item_requests
    );
}
