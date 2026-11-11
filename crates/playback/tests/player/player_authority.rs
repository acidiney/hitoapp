#[path = "../fixtures/authority_outage.rs"]
mod authority_outage;
use hito_core::test_support::playback as fixture;
use hito_playback::{Player, PlayerAction, PlayerCommand, PlayerError};
use std::time::Duration;
use uuid::Uuid;

#[tokio::test]
async fn temporary_authority_failure_preserves_the_queue_lease_and_accepted_receipts() {
    let f = fixture::LocalPlayback::start().await;
    let (player, owner) = Player::start(f.services.clone());
    let attached = player
        .claim(&f.actor, player.inspect(&f.actor).await.unwrap().epoch)
        .await
        .unwrap();
    let command = PlayerCommand {
        id: Uuid::new_v4(),
        lease: attached.lease,
        sequence: 1,
        revision: 0,
        action: PlayerAction::Replace {
            recordings: vec![Uuid::parse_str(&f.recording).unwrap(); 2],
            start: 0,
        },
    };
    let receipt = player.submit(&f.actor, command.clone()).await.unwrap();
    let page = player
        .queue(&f.actor, attached.lease, receipt.revision, 0, 10)
        .await
        .unwrap();
    let ids = page
        .entries
        .iter()
        .map(|entry| entry.id)
        .collect::<Vec<_>>();
    let mut state = player.subscribe(&f.actor, attached.lease).await.unwrap();
    let outage = authority_outage::Outage::start(f.directory.path()).await;
    tokio::time::timeout(
        Duration::from_secs(3),
        state.wait_for(|value| value.error == Some(PlayerError::Unavailable)),
    )
    .await
    .unwrap()
    .expect("temporary authority failure must keep the owned observation open");
    assert_eq!(state.borrow().queue_len, 2);
    assert_eq!(state.borrow().revision, receipt.revision);
    let next = PlayerCommand {
        id: Uuid::new_v4(),
        lease: attached.lease,
        sequence: 2,
        revision: receipt.revision,
        action: PlayerAction::Next {},
    };
    assert_eq!(
        player.submit(&f.actor, next.clone()).await,
        Err(PlayerError::Unavailable)
    );
    outage.restore().await;
    tokio::time::timeout(
        Duration::from_secs(3),
        state.wait_for(|value| value.error.is_none()),
    )
    .await
    .unwrap()
    .unwrap();
    let seen = player.inspect(&f.actor).await.unwrap();
    assert_eq!(seen.epoch, attached.epoch);
    assert_eq!(seen.state.unwrap().current.unwrap().id, ids[0]);
    assert_eq!(player.submit(&f.actor, command).await.unwrap(), receipt);
    let advanced = player.submit(&f.actor, next).await.unwrap();
    let after = player
        .queue(&f.actor, attached.lease, advanced.revision, 0, 10)
        .await
        .unwrap();
    assert_eq!(
        after
            .entries
            .iter()
            .map(|entry| entry.id)
            .collect::<Vec<_>>(),
        ids
    );
    assert_eq!(
        player
            .inspect(&f.actor)
            .await
            .unwrap()
            .state
            .unwrap()
            .current
            .unwrap()
            .id,
        ids[1]
    );
    owner.close().await.unwrap();
}

#[tokio::test]
async fn failed_queue_authorization_suspends_mutation_until_the_selected_recording_can_be_checked()
{
    let f = fixture::LocalPlayback::start().await;
    let (player, owner) = Player::start(f.services.clone());
    let attached = player
        .claim(&f.actor, player.inspect(&f.actor).await.unwrap().epoch)
        .await
        .unwrap();
    player
        .submit(
            &f.actor,
            PlayerCommand {
                id: Uuid::new_v4(),
                lease: attached.lease,
                sequence: 1,
                revision: 0,
                action: PlayerAction::Replace {
                    recordings: vec![Uuid::parse_str(&f.recording).unwrap(); 2],
                    start: 0,
                },
            },
        )
        .await
        .unwrap();
    let mut state = player.subscribe(&f.actor, attached.lease).await.unwrap();
    let current = state.borrow().current;
    let database = sqlx::SqlitePool::connect_with(
        sqlx::sqlite::SqliteConnectOptions::new().filename(f.directory.path().join("hito.db")),
    )
    .await
    .unwrap();
    let sqlite: String = sqlx::query_scalar("SELECT sqlite_version()")
        .fetch_one(&database)
        .await
        .unwrap();
    eprintln!("authority fixture SQLite {sqlite}");
    let before = database_snapshot(&database).await;
    let outage = authority_outage::Outage::recordings(f.directory.path()).await;
    f.services.identity.check_session(&f.actor).await.unwrap();
    assert!(matches!(
        player.queue(&f.actor, attached.lease, 1, 0, 10).await,
        Err(PlayerError::Unavailable)
    ));
    tokio::time::timeout(
        Duration::from_secs(2),
        state.wait_for(|state| state.error == Some(PlayerError::Unavailable)),
    )
    .await
    .unwrap()
    .unwrap();
    tokio::time::sleep(Duration::from_millis(600)).await;
    assert_eq!(state.borrow().error, Some(PlayerError::Unavailable));
    assert_eq!(state.borrow().current, current);
    assert_eq!(
        player
            .submit(
                &f.actor,
                PlayerCommand {
                    id: Uuid::new_v4(),
                    lease: attached.lease,
                    sequence: 2,
                    revision: 1,
                    action: PlayerAction::Next {}
                }
            )
            .await,
        Err(PlayerError::Unavailable)
    );
    outage.restore().await;
    assert!(
        before == database_snapshot(&database).await,
        "recording authority outage must restore the exact schema and rows"
    );
    let violations = sqlx::query("PRAGMA foreign_key_check")
        .fetch_all(&database)
        .await
        .unwrap();
    assert!(violations.is_empty());
    let integrity: String = sqlx::query_scalar("PRAGMA integrity_check")
        .fetch_one(&database)
        .await
        .unwrap();
    assert_eq!(integrity, "ok");
    database.close().await;
    tokio::time::timeout(
        Duration::from_secs(3),
        state.wait_for(|state| state.error.is_none()),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(state.borrow().current, current);
    assert_eq!(state.borrow().revision, 1);
    assert_eq!(
        player.inspect(&f.actor).await.unwrap().epoch,
        attached.epoch
    );
    owner.close().await.unwrap();
}

#[derive(PartialEq, Eq)]
struct DatabaseSnapshot {
    schema: Vec<(String, String, String, i64, Option<String>)>,
    rows: Vec<(String, Vec<Vec<String>>)>,
}

async fn database_snapshot(database: &sqlx::SqlitePool) -> DatabaseSnapshot {
    use sqlx::Row;
    let foreign_keys: i64 = sqlx::query_scalar("PRAGMA foreign_keys")
        .fetch_one(database)
        .await
        .unwrap();
    let ignored_checks: i64 = sqlx::query_scalar("PRAGMA ignore_check_constraints")
        .fetch_one(database)
        .await
        .unwrap();
    assert_eq!((foreign_keys, ignored_checks), (1, 0));
    let schema: Vec<(String, String, String, i64, Option<String>)> = sqlx::query_as(
        "SELECT type,name,tbl_name,rootpage,sql FROM sqlite_schema ORDER BY type,name",
    )
    .fetch_all(database)
    .await
    .unwrap();
    let encode = |value: &str| {
        format!(
            "typeof({value}) || ':' || CASE typeof({value}) WHEN 'text' THEN hex(CAST({value} AS BLOB)) ELSE quote({value}) END"
        )
    };
    let mut rows = Vec::new();
    for (_, table, _, _, _) in schema.iter().filter(|object| object.0 == "table") {
        let columns = sqlx::query(&format!("PRAGMA table_xinfo(\"{table}\")"))
            .fetch_all(database)
            .await
            .unwrap();
        let without_rowid: i64 =
            sqlx::query_scalar("SELECT wr FROM pragma_table_list WHERE schema='main' AND name=?")
                .bind(table)
                .fetch_one(database)
                .await
                .unwrap();
        let mut select = Vec::new();
        if without_rowid == 0
            && let Some(alias) = ["rowid", "_rowid_", "oid"].into_iter().find(|alias| {
                !columns
                    .iter()
                    .any(|column| column.get::<String, _>("name").eq_ignore_ascii_case(alias))
            })
        {
            select.push(encode(alias));
        }
        select.extend(
            columns
                .iter()
                .filter(|column| matches!(column.get::<i64, _>("hidden"), 0 | 2 | 3))
                .map(|column| encode(&format!("\"{}\"", column.get::<String, _>("name")))),
        );
        let select = select.join(",");
        let values = sqlx::query(&format!("SELECT {select} FROM \"{table}\""))
            .fetch_all(database)
            .await
            .unwrap();
        let mut values: Vec<Vec<String>> = values
            .iter()
            .map(|row| (0..row.len()).map(|index| row.get(index)).collect())
            .collect();
        values.sort();
        rows.push((table.clone(), values));
    }
    DatabaseSnapshot { schema, rows }
}

#[tokio::test]
async fn database_snapshot_detects_row_identity_without_changing_primary_keys() {
    let directory = tempfile::tempdir().unwrap();
    let database = sqlx::SqlitePool::connect_with(
        sqlx::sqlite::SqliteConnectOptions::new()
            .filename(directory.path().join("snapshot.db"))
            .create_if_missing(true),
    )
    .await
    .unwrap();
    sqlx::raw_sql(
        "CREATE TABLE identified(id TEXT PRIMARY KEY, value INTEGER NOT NULL);
         CREATE TABLE keyed(id TEXT PRIMARY KEY, value INTEGER NOT NULL) WITHOUT ROWID;
         INSERT INTO identified(rowid,id,value) VALUES(7,'stable',5);
         INSERT INTO keyed(id,value) VALUES('stable',5);",
    )
    .execute(&database)
    .await
    .unwrap();
    let before = database_snapshot(&database).await;
    sqlx::query("UPDATE identified SET rowid=8 WHERE id='stable'")
        .execute(&database)
        .await
        .unwrap();
    let after = database_snapshot(&database).await;
    assert!(
        before != after,
        "a snapshot must detect an implicit row identity change"
    );
    assert!(
        before.rows.iter().find(|row| row.0 == "keyed")
            == after.rows.iter().find(|row| row.0 == "keyed"),
        "a table without rowids must retain its key and values"
    );
    database.close().await;
}

#[tokio::test]
async fn database_snapshot_includes_stored_and_virtual_generated_values() {
    let directory = tempfile::tempdir().unwrap();
    let database = sqlx::SqlitePool::connect_with(
        sqlx::sqlite::SqliteConnectOptions::new()
            .filename(directory.path().join("snapshot.db"))
            .create_if_missing(true),
    )
    .await
    .unwrap();
    sqlx::raw_sql(
        "CREATE TABLE generated(id TEXT PRIMARY KEY, value INTEGER NOT NULL,
            stored INTEGER GENERATED ALWAYS AS (value*2) STORED,
            calculated INTEGER GENERATED ALWAYS AS (value*3) VIRTUAL);
         INSERT INTO generated(rowid,id,value) VALUES(7,'stable',5);",
    )
    .execute(&database)
    .await
    .unwrap();
    let snapshot = database_snapshot(&database).await;
    let values = &snapshot
        .rows
        .iter()
        .find(|row| row.0 == "generated")
        .unwrap()
        .1;
    assert!(
        values.len() == 1 && values[0].iter().any(|value| value == "integer:10"),
        "stored generated values must be captured"
    );
    assert!(
        values[0].iter().any(|value| value == "integer:15"),
        "virtual generated values must be captured"
    );
    database.close().await;
}

#[tokio::test]
async fn database_snapshot_detects_text_after_an_embedded_nul_without_changing_identity() {
    let directory = tempfile::tempdir().unwrap();
    let database = sqlx::SqlitePool::connect_with(
        sqlx::sqlite::SqliteConnectOptions::new()
            .filename(directory.path().join("snapshot.db"))
            .create_if_missing(true),
    )
    .await
    .unwrap();
    sqlx::raw_sql(
        "CREATE TABLE cells(id TEXT PRIMARY KEY, value TEXT NOT NULL,
            whole INTEGER, precise REAL, bytes BLOB, absent TEXT);
         INSERT INTO cells(rowid,id,value,whole,precise,bytes,absent)
            VALUES(7,'key','stable',9223372036854775807,1.2345678901234567,X'0001FF',NULL);",
    )
    .execute(&database)
    .await
    .unwrap();
    let before = database_snapshot(&database).await;
    sqlx::query("UPDATE cells SET value=value || char(0) || 'suffix' WHERE id='key'")
        .execute(&database)
        .await
        .unwrap();
    let after = database_snapshot(&database).await;
    assert!(
        before != after,
        "a snapshot must detect text following an embedded NUL"
    );
    assert!(
        before.schema == after.schema,
        "the sensitivity change must retain the schema"
    );
    let cells = &before.rows.iter().find(|row| row.0 == "cells").unwrap().1[0];
    let changed_cells = &after.rows.iter().find(|row| row.0 == "cells").unwrap().1[0];
    assert!(
        cells[..2] == changed_cells[..2],
        "the sensitivity change must retain the row identity and primary key"
    );
    assert!(
        cells
            .iter()
            .any(|value| value == "integer:9223372036854775807")
    );
    assert!(cells.iter().any(|value| value == "blob:X'0001FF'"));
    assert!(cells.iter().any(|value| value == "null:NULL"));
    let precise = cells
        .iter()
        .find_map(|value| value.strip_prefix("real:"))
        .unwrap()
        .parse::<f64>()
        .unwrap();
    assert!(precise.to_bits() == 1.2345678901234567_f64.to_bits());
    database.close().await;
}
