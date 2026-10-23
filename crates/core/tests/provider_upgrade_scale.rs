#[path = "fixtures/legacy_library.rs"]
mod legacy;
use hito_core::test_support::support;
use sqlx::{migrate::Migrator, sqlite::SqliteConnectOptions};
use std::{
    borrow::Cow,
    time::{Duration, Instant},
};

#[tokio::test]
async fn populated_provider_upgrade_preserves_one_hundred_thousand_collection_references() {
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
        migrations: Cow::Owned(all.iter().filter(|m| m.version <= 18).cloned().collect()),
        ..Migrator::DEFAULT
    }
    .run(&pool)
    .await
    .unwrap();
    let mut tx = pool.begin().await.unwrap();
    sqlx::query(
        "WITH RECURSIVE n(i) AS (VALUES(1) UNION ALL SELECT i+1 FROM n WHERE i < 100000)
        INSERT INTO recordings(id,library_id,title,artist,initial_file_checksum)
        SELECT 'scale-'||i,?,'Scale track '||i,'Scale artist','scale-checksum-'||i FROM n",
    )
    .bind(legacy::LIBRARY)
    .execute(&mut *tx)
    .await
    .unwrap();
    sqlx::query("INSERT INTO sources(id,recording_id,provider,provider_id,ready) SELECT id,id,'fixture',id,0 FROM recordings WHERE id LIKE 'scale-%'")
        .execute(&mut *tx).await.unwrap();
    sqlx::query("INSERT INTO metadata_observations(id,source_id,recording_id,provider,origin,inspection_version,title,artist,duration_ms)
        SELECT id,id,id,'fixture','legacy_normalized',0,title,artist,0 FROM recordings WHERE id LIKE 'scale-%'")
        .execute(&mut *tx).await.unwrap();
    sqlx::query(
        "INSERT INTO source_metadata(source_id,title,artist,duration_ms,observation_id)
        SELECT id,title,artist,0,id FROM recordings WHERE id LIKE 'scale-%'",
    )
    .execute(&mut *tx)
    .await
    .unwrap();
    sqlx::query("UPDATE recordings SET metadata_observation_id=id WHERE id LIKE 'scale-%'")
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("INSERT INTO likes(user_id,recording_id,liked) SELECT 'legacy-admin',id,1 FROM recordings WHERE id LIKE 'scale-%'")
        .execute(&mut *tx).await.unwrap();
    sqlx::query("INSERT INTO listening_progress(user_id,recording_id,position_ms,speed) SELECT 'legacy-admin',id,42,1.25 FROM recordings WHERE id LIKE 'scale-%'")
        .execute(&mut *tx).await.unwrap();
    sqlx::query("INSERT INTO playlist_entries(playlist_id,id,recording_id,position) SELECT ?,id,id,rowid FROM recordings WHERE id LIKE 'scale-%'")
        .bind(legacy::PLAYLIST).execute(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    eprintln!("Populated upgrade fixture ready");
    let started = Instant::now();
    all.run(&pool).await.unwrap();
    let elapsed = started.elapsed();
    eprintln!("Populated 100000-track provider migration: {elapsed:?}");
    assert!(
        elapsed < Duration::from_secs(30),
        "Provider upgrade exceeded its development smoke-test deadline"
    );
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM recordings r JOIN metadata_observations o ON o.id=r.metadata_observation_id AND o.recording_id=r.id
        JOIN source_metadata m ON m.source_id=r.id AND m.observation_id=o.id
        JOIN likes l ON l.recording_id=r.id AND l.user_id='legacy-admin' AND l.liked=1
        JOIN listening_progress p ON p.recording_id=r.id AND p.user_id=l.user_id AND p.position_ms=42 AND p.speed=1.25
        JOIN playlist_entries e ON e.recording_id=r.id AND e.playlist_id=? WHERE r.id LIKE 'scale-%'")
        .bind(legacy::PLAYLIST).fetch_one(&pool).await.unwrap();
    assert_eq!(count, 100000);
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
    pool.close().await;
    let (hito, actor) = fixture.open().await;
    assert_eq!(
        support::audio_bytes(
            hito.playback
                .audio_file(&actor, legacy::SOURCE)
                .await
                .unwrap()
        ),
        std::fs::read(&fixture.original).unwrap()
    );
    let page = hito
        .catalog
        .recordings(
            &actor,
            hito_core::Search {
                q: Some("Scale track".into()),
                liked: Some(true),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(page.total, 100000);
}
