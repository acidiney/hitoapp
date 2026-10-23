use hito_core::test_support::playlist_service as fixture;
use hito_core::test_support::support;

use hito_core::{Action, Error, FollowProviderPlaylist, PlaylistFollowInterval};
use serde_json::json;
use sqlx::{Connection, SqliteConnection, sqlite::SqliteConnectOptions};
use std::time::Duration;
use uuid::Uuid;

async fn make_due(database: &mut SqliteConnection, playlist: &str) {
    let changed = sqlx::query("UPDATE playlist_follows SET next_refresh_at = unixepoch() - 1 WHERE playlist_id = ? AND active = 1")
        .bind(playlist)
        .execute(database)
        .await
        .unwrap()
        .rows_affected();
    assert_eq!(changed, 1);
}

#[tokio::test]
async fn deleting_a_gracefully_requeued_follow_prunes_terminal_history_and_preserves_catalog_records()
 {
    let server = fixture::ProviderServer::start().await;
    let (directory, hito, actor, _shared, _music) =
        support::library("follow-deleted-history").await;
    let installation = fixture::connect(&hito, &actor, &server, &["playlists"]).await;
    fixture::playlist(
        &server,
        "playlist/deleted-history",
        "last-good",
        vec![
            json!({"item": {"kind": "recording", "recording": {
                "id": "recording/retained", "title": "Retained recording", "artist": "Fixture artist"
            }}}),
            json!({"item": {"kind": "unknown"}}),
        ],
    )
    .await;
    let creation_id = Uuid::new_v4();
    let request = || FollowProviderPlaylist {
        request_id: creation_id,
        installation_revision: installation.revision,
        playlist_reference: "https://provider.invalid/deleted-history-selection".into(),
        interval: PlaylistFollowInterval::Hourly,
    };
    let followed = hito
        .collection
        .follow_provider_playlist(&actor, &installation.id, request())
        .await
        .unwrap();
    let playlist = followed.playlist_id.to_string();
    let mut database = SqliteConnection::connect_with(
        &SqliteConnectOptions::new()
            .filename(directory.path().join("hito.db"))
            .foreign_keys(true),
    )
    .await
    .unwrap();
    for _ in 0..32 {
        make_due(&mut database, &playlist).await;
        assert!(
            hito.collection
                .run_next_follow_refresh_until(std::future::pending())
                .await
                .unwrap()
        );
    }
    let last_good =
        serde_json::to_value(hito.collection.playlist(&actor, &playlist).await.unwrap()).unwrap();
    let catalog = serde_json::to_value(
        hito.catalog
            .recordings(&actor, Default::default())
            .await
            .unwrap(),
    )
    .unwrap();
    let complete: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM playlist_follow_jobs WHERE playlist_id = ? AND status = 'complete'",
    )
    .bind(&playlist)
    .fetch_one(&mut database)
    .await
    .unwrap();
    assert_eq!(complete, 32);
    make_due(&mut database, &playlist).await;
    let gate = server.hold("v1/playlists/detail").await;
    let (stop, stopped) = tokio::sync::oneshot::channel::<()>();
    let worker = {
        let collection = hito.collection.clone();
        tokio::spawn(async move {
            collection
                .run_next_follow_refresh_until(async {
                    let _ = stopped.await;
                })
                .await
        })
    };
    let entered = tokio::time::timeout(Duration::from_secs(2), gate.entered()).await;
    let stopping = stop.send(());
    let joined = tokio::time::timeout(Duration::from_secs(2), worker).await;
    gate.open();
    entered.expect("The real scheduled worker must enter its held provider detail request");
    assert!(stopping.is_ok());
    assert!(
        joined
            .expect("Graceful shutdown must join while the provider response remains held")
            .unwrap()
            .unwrap()
    );
    let (queued_job, attempts, lease): (String, i64, Option<String>) = sqlx::query_as("SELECT id,attempts,lease_token FROM playlist_follow_jobs WHERE playlist_id = ? AND status = 'queued'")
        .bind(&playlist)
        .fetch_one(&mut database)
        .await
        .unwrap();
    assert_eq!(
        attempts, 0,
        "Graceful cancellation must refund the unfinished attempt"
    );
    assert!(lease.is_none());
    assert_eq!(
        serde_json::to_value(hito.collection.playlist(&actor, &playlist).await.unwrap()).unwrap(),
        last_good
    );
    let detail_requests = server.requests("v1/playlists/detail").await.len();
    let item_requests = server.requests("v1/playlists/items").await.len();

    support::synchronize(
        &hito,
        &actor,
        vec![Action::PlaylistDelete {
            playlist_id: followed.playlist_id,
        }],
    )
    .await;
    let pending: i64 = sqlx::query_scalar("SELECT count(*) FROM playlist_follow_jobs WHERE playlist_id = ? AND status IN ('queued','running')")
        .bind(&playlist)
        .fetch_one(&mut database)
        .await
        .unwrap();
    assert_eq!(pending, 0);
    let cancelled: String =
        sqlx::query_scalar("SELECT status FROM playlist_follow_jobs WHERE id = ?")
            .bind(&queued_job)
            .fetch_one(&mut database)
            .await
            .unwrap();
    assert_eq!(cancelled, "cancelled");
    assert!(matches!(
        hito.collection.playlist(&actor, &playlist).await,
        Err(Error::NotFound)
    ));
    let permanent = hito
        .collection
        .follow_provider_playlist(&actor, &installation.id, request())
        .await
        .unwrap();
    assert_eq!(permanent.playlist_id, followed.playlist_id);
    assert!(hito.collection.playlists(&actor).await.unwrap().is_empty());
    assert_eq!(
        serde_json::to_value(
            hito.catalog
                .recordings(&actor, Default::default())
                .await
                .unwrap()
        )
        .unwrap(),
        catalog
    );
    assert_eq!(
        server.requests("v1/playlists/detail").await.len(),
        detail_requests
    );
    assert_eq!(
        server.requests("v1/playlists/items").await.len(),
        item_requests
    );
    let manual_receipts: i64 =
        sqlx::query_scalar("SELECT count(*) FROM playlist_follow_refresh_receipts")
            .fetch_one(&mut database)
            .await
            .unwrap();
    assert_eq!(manual_receipts, 0);
    let unreferenced: i64 = sqlx::query_scalar("SELECT count(*) FROM playlist_follow_jobs job
        WHERE job.playlist_id = ? AND job.status IN ('complete','failed','cancelled')
            AND NOT EXISTS(SELECT 1 FROM playlist_follow_refresh_receipts receipt WHERE receipt.job_id = job.id)")
        .bind(&playlist)
        .fetch_one(&mut database)
        .await
        .unwrap();
    assert!(
        unreferenced <= 32,
        "Deleting a gracefully requeued scheduled follow must prune terminal history; observed {unreferenced}"
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
