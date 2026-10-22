use hito_core::test_support::playback as fixture;
use hito_core::test_support::support;
use std::time::Duration;

#[tokio::test]
async fn preferred_playback_excludes_unavailable_history_and_rechecks_membership_and_session() {
    let f = fixture::LocalPlayback::start().await;
    let pool = sqlx::SqlitePool::connect_with(
        sqlx::sqlite::SqliteConnectOptions::new()
            .filename(f.directory.path().join("hito.db"))
            .foreign_keys(true),
    )
    .await
    .unwrap();
    sqlx::query(
        "WITH RECURSIVE n(i) AS (VALUES(1) UNION ALL SELECT i+1 FROM n WHERE i<100000)
        INSERT INTO sources(id,recording_id,provider,provider_id,device_id,ready)
        SELECT 'history-'||i,?,'local','history-'||i,'playback',0 FROM n",
    )
    .bind(&f.recording)
    .execute(&pool)
    .await
    .unwrap();
    for analyzed in [false, true] {
        if analyzed {
            sqlx::query("ANALYZE").execute(&pool).await.unwrap();
        }
        let grant = tokio::time::timeout(
            Duration::from_secs(2),
            f.services
                .catalog
                .preferred_playback_source(&f.actor, &f.recording),
        )
        .await
        .unwrap()
        .unwrap();
        assert!(grant.is_local());
        assert_eq!(
            support::audio_bytes(grant.open_file().await.unwrap()),
            f.original
        );
        let mut candidates = f
            .services
            .catalog
            .playback_candidates(&f.actor, &f.recording)
            .await
            .unwrap();
        let grant = candidates.next().await.unwrap().unwrap();
        assert_eq!(
            support::audio_bytes(grant.open_file().await.unwrap()),
            f.original
        );
        assert!(candidates.next().await.unwrap().is_none());
    }
    f.services
        .identity
        .set_library_access(&f.actor, &f.library, &f.actor.user().id, false)
        .await
        .unwrap();
    assert!(matches!(
        f.services
            .catalog
            .preferred_playback_source(&f.actor, &f.recording)
            .await,
        Err(hito_core::Error::NotFound)
    ));
    f.services
        .identity
        .set_library_access(&f.actor, &f.library, &f.actor.user().id, true)
        .await
        .unwrap();
    let folder = f
        .services
        .storage
        .folders(&f.actor, &f.library)
        .await
        .unwrap()
        .remove(0);
    f.services
        .storage
        .remove_folder(&f.actor, &folder.id, folder.revision)
        .await
        .unwrap();
    assert!(matches!(
        f.services
            .catalog
            .preferred_playback_source(&f.actor, &f.recording)
            .await,
        Err(hito_core::Error::NotFound)
    ));
    f.services.identity.logout(&f.token).await.unwrap();
    assert!(matches!(
        f.services
            .catalog
            .preferred_playback_source(&f.actor, &f.recording)
            .await,
        Err(hito_core::Error::Unauthorized)
    ));
    pool.close().await;
}
