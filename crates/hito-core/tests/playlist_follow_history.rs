#[path = "fixtures/playlist_service.rs"]
mod fixture;
mod support;

use hito_core::{
    Action, Error, FollowProviderPlaylist, PlaylistFollowInterval, RefreshFollowedPlaylist,
};
use serde_json::json;
use sqlx::{Connection, SqliteConnection, sqlite::SqliteConnectOptions};
use uuid::Uuid;

#[tokio::test]
async fn expiring_manual_receipts_trims_deleted_follow_history_without_removing_pinned_or_pending_jobs()
 {
    let server = fixture::ProviderServer::start().await;
    let (directory, hito, actor, _shared, _music) = support::library("follow-history").await;
    let installation = fixture::connect(&hito, &actor, &server, &["playlists"]).await;
    fixture::playlist(
        &server,
        "playlist/history",
        "initial",
        vec![
            json!({"item": {"kind": "recording", "recording": {
                "id": "recording/retained", "title": "Retained recording", "artist": "Fixture artist"
            }}}),
            json!({"item": {"kind": "unknown"}}),
        ],
    )
    .await;
    let creation_id = Uuid::new_v4();
    let deleted_request = || FollowProviderPlaylist {
        request_id: creation_id,
        installation_revision: installation.revision,
        playlist_reference: "https://provider.invalid/deleted-history-selection".into(),
        interval: PlaylistFollowInterval::Hourly,
    };
    let deleted = hito
        .collection
        .follow_provider_playlist(&actor, &installation.id, deleted_request())
        .await
        .unwrap();
    let active = hito
        .collection
        .follow_provider_playlist(
            &actor,
            &installation.id,
            FollowProviderPlaylist {
                request_id: Uuid::new_v4(),
                installation_revision: installation.revision,
                playlist_reference: "https://provider.invalid/active-history-selection".into(),
                interval: PlaylistFollowInterval::Hourly,
            },
        )
        .await
        .unwrap();
    let deleted_playlist = deleted.playlist_id.to_string();
    let active_playlist = active.playlist_id.to_string();
    let mut completed = Vec::new();
    for _ in 0..34 {
        let request_id = Uuid::new_v4();
        let receipt = hito
            .collection
            .enqueue_follow_refresh(
                &actor,
                &deleted_playlist,
                RefreshFollowedPlaylist { request_id },
            )
            .await
            .unwrap();
        assert!(
            hito.collection
                .run_next_follow_refresh_until(std::future::pending())
                .await
                .unwrap()
        );
        completed.push((request_id, receipt.job_id));
    }
    assert_eq!(
        completed
            .iter()
            .map(|(_, job)| *job)
            .collect::<std::collections::HashSet<_>>()
            .len(),
        34
    );
    let (retained_request, retained_job) = *completed.last().unwrap();
    let pending_request = Uuid::new_v4();
    let pending = hito
        .collection
        .enqueue_follow_refresh(
            &actor,
            &active_playlist,
            RefreshFollowedPlaylist {
                request_id: pending_request,
            },
        )
        .await
        .unwrap();
    support::synchronize(
        &hito,
        &actor,
        vec![Action::PlaylistDelete {
            playlist_id: deleted.playlist_id,
        }],
    )
    .await;
    let active_before = serde_json::to_value(
        hito.collection
            .playlist(&actor, &active_playlist)
            .await
            .unwrap(),
    )
    .unwrap();
    let catalog_before = serde_json::to_value(
        hito.catalog
            .recordings(&actor, Default::default())
            .await
            .unwrap(),
    )
    .unwrap();
    let mut database = SqliteConnection::connect_with(
        &SqliteConnectOptions::new()
            .filename(directory.path().join("hito.db"))
            .foreign_keys(true),
    )
    .await
    .unwrap();
    let aged = sqlx::query(
        "UPDATE playlist_follow_refresh_receipts SET created_at = unixepoch() - 691200
        WHERE user_id = ? AND ((playlist_id = ? AND request_id != ?) OR request_id = ?)",
    )
    .bind(&actor.user().id)
    .bind(&deleted_playlist)
    .bind(retained_request.to_string())
    .bind(pending_request.to_string())
    .execute(&mut database)
    .await
    .unwrap()
    .rows_affected();
    assert_eq!(aged, 34);
    server.offline().await;
    let detail_requests = server.requests("v1/playlists/detail").await.len();
    let item_requests = server.requests("v1/playlists/items").await.len();
    let fresh = hito
        .collection
        .enqueue_follow_refresh(
            &actor,
            &active_playlist,
            RefreshFollowedPlaylist {
                request_id: Uuid::new_v4(),
            },
        )
        .await
        .unwrap();
    assert_eq!(fresh.job_id, pending.job_id);
    let unreferenced: i64 = sqlx::query_scalar("SELECT count(*) FROM playlist_follow_jobs job
        WHERE job.playlist_id = ? AND job.status IN ('complete','failed','cancelled')
            AND NOT EXISTS(SELECT 1 FROM playlist_follow_refresh_receipts receipt WHERE receipt.job_id = job.id)")
        .bind(&deleted_playlist)
        .fetch_one(&mut database)
        .await
        .unwrap();
    assert!(
        unreferenced <= 32,
        "Global receipt expiry must trim unreferenced terminal history for deleted follows that cannot finish another worker job; observed {unreferenced}"
    );
    let preserved = hito
        .collection
        .enqueue_follow_refresh(
            &actor,
            &deleted_playlist,
            RefreshFollowedPlaylist {
                request_id: retained_request,
            },
        )
        .await
        .unwrap();
    assert_eq!(preserved.job_id, retained_job);
    let pending_binding: String = sqlx::query_scalar(
        "SELECT job_id FROM playlist_follow_refresh_receipts WHERE user_id = ? AND request_id = ?",
    )
    .bind(&actor.user().id)
    .bind(pending_request.to_string())
    .fetch_one(&mut database)
    .await
    .unwrap();
    assert_eq!(pending_binding, pending.job_id.to_string());
    let pending_status: String =
        sqlx::query_scalar("SELECT status FROM playlist_follow_jobs WHERE id = ?")
            .bind(pending.job_id.to_string())
            .fetch_one(&mut database)
            .await
            .unwrap();
    assert_eq!(pending_status, "queued");
    let permanent = hito
        .collection
        .follow_provider_playlist(&actor, &installation.id, deleted_request())
        .await
        .unwrap();
    assert_eq!(permanent.playlist_id, deleted.playlist_id);
    assert!(matches!(
        hito.collection.playlist(&actor, &deleted_playlist).await,
        Err(Error::NotFound)
    ));
    assert_eq!(
        serde_json::to_value(
            hito.collection
                .playlist(&actor, &active_playlist)
                .await
                .unwrap()
        )
        .unwrap(),
        active_before
    );
    assert_eq!(
        serde_json::to_value(
            hito.catalog
                .recordings(&actor, Default::default())
                .await
                .unwrap()
        )
        .unwrap(),
        catalog_before
    );
    assert_eq!(
        server.requests("v1/playlists/detail").await.len(),
        detail_requests
    );
    assert_eq!(
        server.requests("v1/playlists/items").await.len(),
        item_requests
    );
    assert!(
        sqlx::query("PRAGMA foreign_key_check")
            .fetch_all(&mut database)
            .await
            .unwrap()
            .is_empty()
    );
    database.close().await.unwrap();
}
