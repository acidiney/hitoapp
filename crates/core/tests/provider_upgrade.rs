#[path = "fixtures/legacy_library.rs"]
mod legacy;
use hito_core::test_support::support;
use sqlx::{migrate::Migrator, sqlite::SqliteConnectOptions};
use std::borrow::Cow;

#[tokio::test]
async fn provider_schema_upgrade_preserves_file_evidence_references_and_search() {
    let fixture = legacy::Legacy::seed().await;
    let pool = sqlx::SqlitePool::connect_with(
        SqliteConnectOptions::new()
            .filename(fixture.directory.path().join("hito.db"))
            .foreign_keys(true),
    )
    .await
    .unwrap();
    let all = sqlx::migrate!("./migrations");
    Migrator {
        migrations: Cow::Owned(
            all.iter()
                .filter(|migration| migration.version <= 18)
                .cloned()
                .collect(),
        ),
        ..Migrator::DEFAULT
    }
    .run(&pool)
    .await
    .unwrap();
    sqlx::query("INSERT INTO metadata_observations(id,source_id,recording_id,provider,origin,inspection_version,title,artist,duration_ms) VALUES ('removed-observation',?,?,'local','legacy_normalized',0,'Removed','Removed',0)").bind(legacy::SOURCE).bind(legacy::RECORDING).execute(&pool).await.unwrap();
    let highwater: i64 =
        sqlx::query_scalar("SELECT seq FROM sqlite_sequence WHERE name='metadata_observations'")
            .fetch_one(&pool)
            .await
            .unwrap();
    sqlx::query("DELETE FROM metadata_observations WHERE id='removed-observation'")
        .execute(&pool)
        .await
        .unwrap();
    let before: String = sqlx::query_scalar("SELECT json_array(rowid,id,library_id,initial_file_checksum,title,artist,metadata_source_id,metadata_observation_id) FROM recordings").fetch_one(&pool).await.unwrap();
    let observations: String = sqlx::query_scalar("SELECT json_group_array(json_array(sequence,id,source_id,recording_id,origin,file_checksum,inspection_version,title,artist)) FROM metadata_observations").fetch_one(&pool).await.unwrap();
    all.run(&pool)
        .await
        .expect("Provider evidence migration must preserve the existing schema");
    pool.close().await;
    let (hito, actor) = fixture.open().await;
    let pool = sqlx::SqlitePool::connect_with(
        SqliteConnectOptions::new()
            .filename(fixture.directory.path().join("hito.db"))
            .foreign_keys(true),
    )
    .await
    .unwrap();
    assert_eq!(sqlx::query_scalar::<_, String>("SELECT json_array(rowid,id,library_id,initial_file_checksum,title,artist,metadata_source_id,metadata_observation_id) FROM recordings").fetch_one(&pool).await.unwrap(), before);
    assert_eq!(sqlx::query_scalar::<_, String>("SELECT json_group_array(json_array(sequence,id,source_id,recording_id,origin,file_checksum,inspection_version,title,artist)) FROM metadata_observations").fetch_one(&pool).await.unwrap(), observations);
    assert_eq!(
        support::audio_bytes(
            hito.playback
                .audio_file(&actor, legacy::SOURCE)
                .await
                .unwrap()
        ),
        std::fs::read(&fixture.original).unwrap()
    );
    let tracks = hito
        .catalog
        .recordings(
            &actor,
            hito_core::Search {
                q: Some("Legacy track".into()),
                liked: Some(true),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(tracks.items[0].id, legacy::RECORDING);
    assert_eq!(
        hito.collection
            .playlist(&actor, legacy::PLAYLIST)
            .await
            .unwrap()
            .entries[0]
            .recording_id
            .as_deref(),
        Some(legacy::RECORDING)
    );
    let mut tx = pool.begin().await.unwrap();
    sqlx::query("INSERT INTO recordings(id,library_id,title,artist,initial_file_checksum,provenance) VALUES ('provider-probe',?,'Provider track','Provider artist',NULL,'provider_response')").bind(legacy::LIBRARY).execute(&mut *tx).await.expect("recordings without file evidence need a real NULL checksum");
    sqlx::query("INSERT INTO metadata_observations(id,recording_id,provider,origin,observed_at,inspection_version,title,artist,duration_ms) VALUES ('provider-observation-probe','provider-probe','org.hito.test','provider_response',unixepoch(),NULL,'Provider track','Provider artist',0)").execute(&mut *tx).await.expect("provider provenance must not invent a local source or inspection version");
    let sequence: i64 = sqlx::query_scalar(
        "SELECT sequence FROM metadata_observations WHERE id='provider-observation-probe'",
    )
    .fetch_one(&mut *tx)
    .await
    .unwrap();
    assert!(sequence > highwater);
    tx.rollback().await.unwrap();
    assert!(
        sqlx::query("PRAGMA foreign_key_check")
            .fetch_all(&pool)
            .await
            .unwrap()
            .is_empty()
    );
    sqlx::query("INSERT INTO recordings_fts(recordings_fts,rank) VALUES ('integrity-check',1)")
        .execute(&pool)
        .await
        .unwrap();
    assert!(hito.healthy().await);
}

#[tokio::test]
async fn provider_upgrade_rejects_broken_references_and_rolls_back_the_rebuild() {
    let fixture = legacy::Legacy::seed().await;
    let pool = sqlx::SqlitePool::connect_with(
        SqliteConnectOptions::new()
            .filename(fixture.directory.path().join("hito.db"))
            .foreign_keys(false),
    )
    .await
    .unwrap();
    let all = sqlx::migrate!("./migrations");
    Migrator {
        migrations: Cow::Owned(
            all.iter()
                .filter(|migration| migration.version <= 18)
                .cloned()
                .collect(),
        ),
        ..Migrator::DEFAULT
    }
    .run(&pool)
    .await
    .unwrap();
    sqlx::query("INSERT INTO listening_progress(user_id,recording_id,position_ms,speed) VALUES ('legacy-admin','missing-recording',0,1)").execute(&pool).await.unwrap();
    assert!(all.run(&pool).await.is_err());
    let nullable: bool = sqlx::query_scalar("SELECT NOT \"notnull\" FROM pragma_table_info('recordings') WHERE name='initial_file_checksum'").fetch_one(&pool).await.unwrap();
    assert!(
        !nullable,
        "a rejected migration must leave the previous schema intact"
    );
    let max: i64 = sqlx::query_scalar("SELECT max(version) FROM _sqlx_migrations WHERE success=1")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(max, 18);
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM recordings WHERE id=?")
            .bind(legacy::RECORDING)
            .fetch_one(&pool)
            .await
            .unwrap(),
        1
    );
}
