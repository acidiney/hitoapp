#[path = "fixtures/legacy_library.rs"]
mod legacy;
use hito_core::test_support::support;
use sqlx::{migrate::Migrator, sqlite::SqliteConnectOptions};
use std::borrow::Cow;

#[tokio::test]
async fn upgrading_existing_memberships_preserves_library_data_and_mints_independent_playback_grants()
 {
    let f = legacy::Legacy::seed().await;
    let options = SqliteConnectOptions::new()
        .filename(f.directory.path().join("hito.db"))
        .foreign_keys(true);
    let pool = sqlx::SqlitePool::connect_with(options.clone())
        .await
        .unwrap();
    let all = sqlx::migrate!("./migrations");
    Migrator {
        migrations: Cow::Owned(all.iter().filter(|m| m.version <= 22).cloned().collect()),
        ..Migrator::DEFAULT
    }
    .run(&pool)
    .await
    .unwrap();
    sqlx::raw_sql("WITH RECURSIVE n(i) AS (VALUES(1) UNION ALL SELECT i+1 FROM n WHERE i<9)
        INSERT INTO users(id,username,password_hash) SELECT 'listener-'||i,'Listener '||i,'unused' FROM n;
        INSERT INTO library_members(library_id,user_id) SELECT '10000000-0000-4000-8000-000000000003',id FROM users WHERE id!='legacy-admin';")
        .execute(&pool).await.unwrap();
    let before: String = sqlx::query_scalar("SELECT json_group_array(json_array(rowid,library_id,user_id)) FROM library_members ORDER BY rowid")
        .fetch_one(&pool).await.unwrap();
    pool.close().await;

    let (services, actor) = f.open().await;
    let grant = services
        .catalog
        .playback_source(&actor, legacy::RECORDING, legacy::SOURCE)
        .await
        .unwrap();
    assert_eq!(
        support::audio_bytes(grant.open_file().await.unwrap()),
        std::fs::read(&f.original).unwrap()
    );
    assert_eq!(
        services
            .collection
            .playlist(&actor, legacy::PLAYLIST)
            .await
            .unwrap()
            .entries[0]
            .entry_id,
        legacy::ENTRY
    );
    let pool = sqlx::SqlitePool::connect_with(options).await.unwrap();
    let after: String = sqlx::query_scalar("SELECT json_group_array(json_array(rowid,library_id,user_id)) FROM library_members ORDER BY rowid")
        .fetch_one(&pool).await.unwrap();
    assert_eq!(before, after);
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT count(DISTINCT id) FROM library_access_grants WHERE length(id)=32"
        )
        .fetch_one(&pool)
        .await
        .unwrap(),
        10
    );
    assert!(
        sqlx::query("PRAGMA foreign_key_check")
            .fetch_all(&pool)
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        sqlx::query_scalar::<_, String>("PRAGMA integrity_check")
            .fetch_one(&pool)
            .await
            .unwrap(),
        "ok"
    );
    pool.close().await;
}
