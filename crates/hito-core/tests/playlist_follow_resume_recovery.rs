#[path = "fixtures/playlist_service.rs"]
mod fixture;
mod support;

use hito_core::{
    Action, FollowProviderPlaylist, PlaylistFollowInterval, PlaylistFollowStatus, PluginRefresh,
    RefreshFollowedPlaylist, RefreshPlugin,
};
use serde_json::{Value, json};
use sqlx::{Connection, SqliteConnection, sqlite::SqliteConnectOptions};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use uuid::Uuid;

fn recording(occurrence: &str, recording: &str) -> Value {
    json!({"occurrence_id": occurrence, "item": {"kind": "recording", "recording": {
        "id": format!("recording/{recording}"), "title": format!("Recording {recording}"),
        "artist": "Owned recovery fixture artist", "duration_ms": 123000
    }}})
}

async fn expire_lease(database: &mut SqliteConnection, job: Uuid, attempt: i64) {
    let aged = sqlx::query("UPDATE playlist_follow_jobs SET lease_until = unixepoch() - 1
        WHERE id = ? AND generation = 1 AND status = 'running' AND attempts = ? AND lease_token IS NOT NULL")
        .bind(job.to_string()).bind(attempt).execute(database).await.unwrap().rows_affected();
    assert_eq!(aged, 1);
}

#[tokio::test]
async fn recovering_an_exhausted_old_lease_preserves_the_resumed_generation_and_starts_its_due_refresh()
 {
    let server = fixture::ProviderServer::start().await;
    let (directory, hito, actor, _shared, _music) =
        support::library("follow-resume-recovery").await;
    let installation = fixture::connect(&hito, &actor, &server, &["playlists"]).await;
    fixture::playlist(
        &server,
        "playlist/resume-recovery",
        "last-good",
        vec![recording("first-a", "a"), recording("duplicate-a", "a")],
    )
    .await;
    let followed = hito
        .collection
        .follow_provider_playlist(
            &actor,
            &installation.id,
            FollowProviderPlaylist {
                request_id: Uuid::new_v4(),
                installation_revision: installation.revision,
                playlist_reference: "https://provider.invalid/owned-resume-recovery".into(),
                interval: PlaylistFollowInterval::Hourly,
            },
        )
        .await
        .unwrap();
    let playlist = followed.playlist_id.to_string();
    let original = hito.collection.playlist(&actor, &playlist).await.unwrap();
    let original_json = serde_json::to_value(&original).unwrap();
    let original_catalog = serde_json::to_value(
        hito.catalog
            .recordings(&actor, Default::default())
            .await
            .unwrap(),
    )
    .unwrap();
    let request_id = Uuid::new_v4();
    let receipt = hito
        .collection
        .enqueue_follow_refresh(&actor, &playlist, RefreshFollowedPlaylist { request_id })
        .await
        .unwrap();
    let receipt_json = serde_json::to_value(&receipt).unwrap();
    let mut database = SqliteConnection::connect_with(
        &SqliteConnectOptions::new()
            .filename(directory.path().join("hito.db"))
            .foreign_keys(true),
    )
    .await
    .unwrap();
    for attempt in 1..=3 {
        let gate = server.hold("v1/playlists/detail").await;
        let collection = hito.collection.clone();
        let worker = tokio::spawn(async move {
            collection
                .run_next_follow_refresh_until(std::future::pending())
                .await
        });
        let entered = tokio::time::timeout(Duration::from_secs(2), gate.entered()).await;
        worker.abort();
        let stopped = tokio::time::timeout(Duration::from_secs(2), worker).await;
        gate.open();
        entered.expect("Each real old-generation attempt must enter its held provider request");
        assert!(
            matches!(stopped.expect("Each aborted public worker must join"), Err(error) if error.is_cancelled())
        );
        let running: (String, i64, i64, bool) = sqlx::query_as(
            "SELECT id,attempts,generation,lease_until > unixepoch() FROM playlist_follow_jobs
            WHERE id = ? AND playlist_id = ? AND status = 'running' AND lease_token IS NOT NULL",
        )
        .bind(receipt.job_id.to_string())
        .bind(&playlist)
        .fetch_one(&mut database)
        .await
        .unwrap();
        assert_eq!(running, (receipt.job_id.to_string(), attempt, 1, true));
        if attempt < 3 {
            expire_lease(&mut database, receipt.job_id, attempt).await;
        }
    }
    assert_eq!(
        hito.collection
            .playlist_follow(&actor, &playlist)
            .await
            .unwrap()
            .status,
        PlaylistFollowStatus::Running
    );
    assert_eq!(server.requests("v1/playlists/detail").await.len(), 5);
    assert_eq!(server.requests("v1/playlists/items").await.len(), 1);

    server
        .reply(
            "v1/manifest",
            json!({
                "id": "org.hito.playlist-fixture", "name": "Reviewed recovery fixture",
                "protocol_version": 1, "capabilities": ["playlists", "search"]
            }),
        )
        .await;
    let PluginRefresh::ReviewRequired { digest, .. } = hito
        .plugins
        .refresh(
            &actor,
            &installation.id,
            RefreshPlugin {
                revision: installation.revision,
                accept_manifest_digest: None,
                credential: Default::default(),
            },
        )
        .await
        .unwrap()
    else {
        panic!("The changed fixture manifest must require explicit review");
    };
    let PluginRefresh::Updated {
        installation: approved,
    } = hito
        .plugins
        .refresh(
            &actor,
            &installation.id,
            RefreshPlugin {
                revision: installation.revision,
                accept_manifest_digest: Some(digest),
                credential: Default::default(),
            },
        )
        .await
        .unwrap()
    else {
        panic!("The exact reviewed fixture permission revision must be accepted");
    };
    assert_eq!(approved.id, installation.id);
    assert_eq!(approved.service_id, installation.service_id);
    let settings = hito.plugins.settings(&actor).await.unwrap();
    let service = settings
        .services
        .iter()
        .find(|service| service.id == approved.service_id)
        .unwrap();
    let paused = hito
        .collection
        .playlist_follow(&actor, &playlist)
        .await
        .unwrap();
    assert_eq!(paused.status, PlaylistFollowStatus::Paused);
    support::synchronize(
        &hito,
        &actor,
        vec![Action::PlaylistFollowResume {
            playlist_id: followed.playlist_id,
            expected_generation: paused.generation,
            installation_revision: approved.revision,
            service_revision: service.revision,
        }],
    )
    .await;
    let resumed = hito
        .collection
        .playlist_follow(&actor, &playlist)
        .await
        .unwrap();
    assert_eq!(resumed.generation, 2);
    assert_eq!(resumed.status, PlaylistFollowStatus::Ready);
    assert!(resumed.error.is_none());
    assert!(
        resumed.next_refresh_at
            <= SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs() as i64
    );
    let charged: (String, i64, i64, bool) = sqlx::query_as("SELECT status,attempts,generation,lease_until > unixepoch() FROM playlist_follow_jobs WHERE id = ?")
        .bind(receipt.job_id.to_string()).fetch_one(&mut database).await.unwrap();
    assert_eq!(charged, ("running".into(), 3, 1, true));
    let replay = hito
        .collection
        .enqueue_follow_refresh(&actor, &playlist, RefreshFollowedPlaylist { request_id })
        .await
        .unwrap();
    assert_eq!(serde_json::to_value(replay).unwrap(), receipt_json);
    let resumed_feed =
        serde_json::to_value(hito.collection.events(&actor, 0).await.unwrap()).unwrap();
    assert_eq!(
        serde_json::to_value(hito.collection.playlist(&actor, &playlist).await.unwrap()).unwrap(),
        original_json
    );

    expire_lease(&mut database, receipt.job_id, 3).await;
    fixture::playlist(
        &server,
        "playlist/resume-recovery",
        "current-generation",
        vec![
            recording("duplicate-a", "a"),
            recording("added-b", "b"),
            recording("first-a", "a"),
        ],
    )
    .await;
    let gate = server.hold("v1/playlists/items").await;
    let recovery = hito
        .collection
        .run_next_follow_refresh_until(std::future::pending());
    tokio::pin!(recovery);
    let mut early_finish = None;
    let entered = tokio::time::timeout(Duration::from_secs(3), async {
        tokio::select! {
            biased;
            _ = gate.entered() => true,
            result = &mut recovery => {
                early_finish = Some(result);
                false
            },
        }
    })
    .await
    .expect("Recovery must finish or enter the owned current-generation provider gate");
    let recovering = hito
        .collection
        .playlist_follow(&actor, &playlist)
        .await
        .unwrap();
    assert_eq!(recovering.generation, 2);
    assert!(
        recovering.error.is_none(),
        "An exhausted old-generation lease must not write refresh_failed onto the resumed generation"
    );
    assert_eq!(
        recovering.next_refresh_at, resumed.next_refresh_at,
        "Old lease recovery must preserve the resumed generation's due schedule"
    );
    assert!(
        entered && early_finish.is_none(),
        "The resumed generation must start its due refresh without another manual enqueue"
    );
    assert_eq!(recovering.status, PlaylistFollowStatus::Running);
    assert_eq!(recovering.snapshot, resumed.snapshot);
    assert_eq!(recovering.last_refreshed_at, resumed.last_refreshed_at);
    assert_eq!(
        serde_json::to_value(hito.collection.playlist(&actor, &playlist).await.unwrap()).unwrap(),
        original_json
    );
    assert_eq!(
        serde_json::to_value(
            hito.catalog
                .recordings(&actor, Default::default())
                .await
                .unwrap()
        )
        .unwrap(),
        original_catalog
    );
    assert_eq!(
        serde_json::to_value(hito.collection.events(&actor, 0).await.unwrap()).unwrap(),
        resumed_feed
    );
    let retired: (String, i64, i64, Option<String>, Option<i64>) = sqlx::query_as("SELECT status,attempts,generation,lease_token,lease_until FROM playlist_follow_jobs WHERE id = ?")
        .bind(receipt.job_id.to_string()).fetch_one(&mut database).await.unwrap();
    assert_eq!(retired, ("failed".into(), 3, 1, None, None));
    let replay = hito
        .collection
        .enqueue_follow_refresh(&actor, &playlist, RefreshFollowedPlaylist { request_id })
        .await
        .unwrap();
    assert_eq!(serde_json::to_value(replay).unwrap(), receipt_json);
    gate.open();
    assert!(
        tokio::time::timeout(Duration::from_secs(3), &mut recovery)
            .await
            .expect("The current-generation refresh must join after provider release")
            .unwrap()
    );
    let completed = hito
        .collection
        .playlist_follow(&actor, &playlist)
        .await
        .unwrap();
    assert_eq!(completed.generation, 2);
    assert_eq!(completed.status, PlaylistFollowStatus::Ready);
    assert_eq!(completed.snapshot, "current-generation");
    assert!(completed.error.is_none());
    let updated = hito.collection.playlist(&actor, &playlist).await.unwrap();
    assert_eq!(updated.playlist.id, original.playlist.id);
    assert_eq!(updated.entries.len(), 3);
    assert_eq!(updated.entries[0].entry_id, original.entries[1].entry_id);
    assert_eq!(updated.entries[2].entry_id, original.entries[0].entry_id);
    assert_eq!(
        updated.entries[0].recording_id,
        original.entries[0].recording_id
    );
    assert_eq!(
        updated.entries[2].recording_id,
        original.entries[0].recording_id
    );
    assert!(
        !original
            .entries
            .iter()
            .any(|entry| entry.entry_id == updated.entries[1].entry_id)
    );
    let feed = serde_json::to_value(hito.collection.events(&actor, 0).await.unwrap()).unwrap();
    let events = feed["operations"].as_array().unwrap();
    let previous_events = resumed_feed["operations"].as_array().unwrap();
    assert_eq!(events.len(), previous_events.len() + 1);
    assert_eq!(&events[..previous_events.len()], previous_events.as_slice());
    assert_eq!(
        events.last().unwrap()["publication"]["kind"],
        "playlist_follow_refreshed"
    );
    assert_eq!(server.requests("v1/sources").await.len(), 0);
    assert_eq!(server.requests("v1/audio").await.len(), 0);
    database.close().await.unwrap();
}
