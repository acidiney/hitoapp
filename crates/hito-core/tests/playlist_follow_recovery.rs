#[path = "fixtures/playlist_service.rs"]
mod fixture;
mod support;

use hito_core::{FollowProviderPlaylist, PlaylistFollowInterval, PlaylistFollowStatus};
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
async fn recovering_an_exhausted_scheduled_lease_trims_terminal_history_without_changing_the_last_good_snapshot()
 {
    let server = fixture::ProviderServer::start().await;
    let (directory, hito, actor, _shared, _music) = support::library("follow-recovery").await;
    let installation = fixture::connect(&hito, &actor, &server, &["playlists"]).await;
    fixture::playlist(
        &server,
        "playlist/recovery",
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
        playlist_reference: "https://provider.invalid/recovery-selection".into(),
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
    let complete: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM playlist_follow_jobs WHERE playlist_id = ? AND status = 'complete'",
    )
    .bind(&playlist)
    .fetch_one(&mut database)
    .await
    .unwrap();
    assert_eq!(complete, 32);
    let last_good =
        serde_json::to_value(hito.collection.playlist(&actor, &playlist).await.unwrap()).unwrap();
    let catalog = serde_json::to_value(
        hito.catalog
            .recordings(&actor, Default::default())
            .await
            .unwrap(),
    )
    .unwrap();
    make_due(&mut database, &playlist).await;
    let mut crashed_job = None;
    for expected_attempt in 1..=3 {
        let gate = server.hold("v1/playlists/detail").await;
        let worker = {
            let collection = hito.collection.clone();
            tokio::spawn(async move {
                collection
                    .run_next_follow_refresh_until(std::future::pending())
                    .await
            })
        };
        let entered = tokio::time::timeout(Duration::from_secs(2), gate.entered()).await;
        worker.abort();
        let stopped = tokio::time::timeout(Duration::from_secs(2), worker).await;
        gate.open();
        entered.expect("The actual scheduled attempt must enter its held provider detail request");
        assert!(
            matches!(stopped.expect("The aborted public worker must join"), Err(error) if error.is_cancelled())
        );
        let (job, attempts): (String, i64) = sqlx::query_as("SELECT id,attempts FROM playlist_follow_jobs WHERE playlist_id = ? AND status = 'running'")
            .bind(&playlist)
            .fetch_one(&mut database)
            .await
            .unwrap();
        assert_eq!(attempts, expected_attempt);
        if let Some(previous) = &crashed_job {
            assert_eq!(&job, previous);
        } else {
            crashed_job = Some(job.clone());
        }
        let aged = sqlx::query("UPDATE playlist_follow_jobs SET lease_until = unixepoch() - 1 WHERE id = ? AND status = 'running' AND attempts = ? AND lease_token IS NOT NULL")
            .bind(&job)
            .bind(expected_attempt)
            .execute(&mut database)
            .await
            .unwrap()
            .rows_affected();
        assert_eq!(aged, 1);
    }
    let detail_requests = server.requests("v1/playlists/detail").await.len();
    let item_requests = server.requests("v1/playlists/items").await.len();
    assert!(
        !hito
            .collection
            .run_next_follow_refresh_until(std::future::pending())
            .await
            .unwrap(),
        "Recovering the exhausted lease must finish without starting another provider attempt"
    );
    let status = hito
        .collection
        .playlist_follow(&actor, &playlist)
        .await
        .unwrap();
    assert_eq!(status.status, PlaylistFollowStatus::Failed);
    assert_eq!(status.snapshot, "last-good");
    assert_eq!(status.error.as_deref(), Some("refresh_failed"));
    assert_eq!(
        serde_json::to_value(hito.collection.playlist(&actor, &playlist).await.unwrap()).unwrap(),
        last_good
    );
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
    let permanent = hito
        .collection
        .follow_provider_playlist(&actor, &installation.id, request())
        .await
        .unwrap();
    assert_eq!(permanent.playlist_id, followed.playlist_id);
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
    assert_eq!(
        manual_receipts, 0,
        "All retained jobs must come from actual scheduled worker claims"
    );
    let unreferenced: i64 = sqlx::query_scalar("SELECT count(*) FROM playlist_follow_jobs job
        WHERE job.playlist_id = ? AND job.status IN ('complete','failed','cancelled')
            AND NOT EXISTS(SELECT 1 FROM playlist_follow_refresh_receipts receipt WHERE receipt.job_id = job.id)")
        .bind(&playlist)
        .fetch_one(&mut database)
        .await
        .unwrap();
    assert!(
        unreferenced <= 32,
        "Exhausted lease recovery must prune terminal history even when no worker reaches ordinary finish; observed {unreferenced}"
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
