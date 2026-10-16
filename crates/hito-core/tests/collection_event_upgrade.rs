#[path = "fixtures/legacy_library.rs"]
mod legacy;
#[path = "fixtures/playlist_service.rs"]
mod provider;
mod support;

use hito_core::{
    Action, CollectionPublication, ImportProviderPlaylist, Operation, SyncBatch, SyncEvent,
};
use serde_json::json;
use sqlx::{migrate::Migrator, sqlite::SqliteConnectOptions};
use std::borrow::Cow;
use uuid::Uuid;

#[tokio::test]
async fn reopening_the_old_ledger_preserves_wire_events_replay_and_deleted_sequence_high_water() {
    let fixture = legacy::Legacy::seed().await;
    let pool = sqlx::SqlitePool::connect_with(
        SqliteConnectOptions::new()
            .filename(fixture.directory.path().join("hito.db"))
            .foreign_keys(true),
    )
    .await
    .unwrap();
    let migrations = sqlx::migrate!("./migrations");
    Migrator {
        migrations: Cow::Owned(
            migrations
                .iter()
                .filter(|migration| migration.version <= 23)
                .cloned()
                .collect(),
        ),
        ..Migrator::DEFAULT
    }
    .run(&pool)
    .await
    .unwrap();
    let mut expected = Vec::new();
    for sequence in [7_i64, 33, 900] {
        let operation = Operation {
            id: Uuid::new_v4(),
            action: Action::PlaylistRename {
                playlist_id: Uuid::parse_str(legacy::PLAYLIST).unwrap(),
                name: "Legacy playlist".into(),
            },
        };
        let payload = serde_json::to_string(&operation).unwrap();
        sqlx::query("INSERT INTO sync_operations(sequence, user_id, device_id, operation_id, payload) VALUES (?, 'legacy-admin', 'prior-client', ?, ?)")
            .bind(sequence).bind(operation.id.to_string()).bind(payload).execute(&pool).await.unwrap();
        if sequence != 900 {
            expected.push(
                json!({"sequence": sequence, "device_id": "prior-client", "operation": operation}),
            );
        }
    }
    sqlx::query("DELETE FROM sync_operations WHERE sequence = 900")
        .execute(&pool)
        .await
        .unwrap();
    let old_high_water: i64 =
        sqlx::query_scalar("SELECT seq FROM sqlite_sequence WHERE name = 'sync_operations'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(old_high_water, 900);
    pool.close().await;

    let (hito, actor) = fixture.open().await;
    let initial = hito.collection.events(&actor, 0).await.unwrap();
    assert_eq!(initial.cursor, 33);
    assert!(!initial.has_more);
    assert_eq!(
        serde_json::to_value(&initial.operations).unwrap(),
        json!(expected)
    );
    let replay: Operation = serde_json::from_value(expected[0]["operation"].clone()).unwrap();
    let replay_id = replay.id;
    assert_eq!(
        hito.collection
            .apply(
                &actor,
                SyncBatch {
                    device_id: "prior-client".into(),
                    operations: vec![replay],
                }
            )
            .await
            .unwrap()
            .acknowledged,
        vec![replay_id]
    );
    let replayed = hito.collection.events(&actor, 0).await.unwrap();
    assert_eq!(replayed.cursor, 33);
    assert_eq!(
        serde_json::to_value(replayed.operations).unwrap(),
        json!(expected)
    );
    let operation_id = Uuid::new_v4();
    hito.collection
        .apply(
            &actor,
            SyncBatch {
                device_id: "upgraded-client".into(),
                operations: vec![Operation {
                    id: operation_id,
                    action: Action::PlaylistRename {
                        playlist_id: Uuid::parse_str(legacy::PLAYLIST).unwrap(),
                        name: "After upgrade".into(),
                    },
                }],
            },
        )
        .await
        .unwrap();
    let retained = hito
        .collection
        .playlist(&actor, legacy::PLAYLIST)
        .await
        .unwrap();
    assert_eq!(retained.playlist.name, "After upgrade");
    assert!(retained.origin.is_none());
    assert_eq!(retained.entries[0].entry_id, legacy::ENTRY);
    assert_eq!(
        retained.entries[0].recording_id.as_deref(),
        Some(legacy::RECORDING)
    );
    assert_eq!(
        support::audio_bytes(
            hito.catalog
                .audio_file(&actor, legacy::SOURCE)
                .await
                .unwrap()
        ),
        std::fs::read(&fixture.original).unwrap()
    );

    let server = provider::ProviderServer::start().await;
    let installed = provider::connect(&hito, &actor, &server, &["playlists"]).await;
    provider::playlist(&server, "playlist/upgraded", "snapshot-one", vec![
        json!({"occurrence_id": "new-entry", "item": {"kind": "recording", "recording": {
            "id": "recording/upgraded", "title": "Imported after upgrade", "artist": "Fixture artist"
        }}}),
    ]).await;
    let imported = hito
        .collection
        .import_provider_playlist(
            &actor,
            &installed.id,
            ImportProviderPlaylist {
                request_id: Uuid::new_v4(),
                installation_revision: installed.revision,
                playlist_reference: "https://provider.invalid/upgrade-selection".into(),
            },
        )
        .await
        .unwrap();
    let following = hito
        .collection
        .events(&actor, old_high_water)
        .await
        .unwrap();
    assert_eq!(following.operations.len(), 2);
    let SyncEvent::Operation {
        sequence: client_sequence,
        device_id,
        operation,
    } = &following.operations[0]
    else {
        panic!("A submitted client action must retain its ordinary event envelope");
    };
    assert!(*client_sequence > old_high_water);
    assert_eq!(device_id, "upgraded-client");
    assert_eq!(operation.id, operation_id);
    let SyncEvent::Publication {
        sequence: publication_sequence,
        publication: CollectionPublication::PlaylistImported { snapshot, .. },
    } = &following.operations[1]
    else {
        panic!("The imported copy must publish a distinct collection event");
    };
    assert!(*publication_sequence > *client_sequence);
    assert_eq!(snapshot.playlist_id, imported.playlist_id);
    assert_eq!(following.cursor, *publication_sequence);
    assert!(!following.has_more);
}
