mod support;

#[tokio::test]
async fn the_latest_folder_status_is_the_most_recently_accepted_scan() {
    let (_dir, services, actor, library, _music) = support::library("server").await;
    for _ in 0..12 {
        let job = services
            .storage
            .enqueue_scan(&actor, &library)
            .await
            .unwrap()
            .remove(0);
        let folder = services
            .storage
            .folders(&actor, &library)
            .await
            .unwrap()
            .remove(0);
        assert_eq!(folder.latest_job.unwrap().id, job.id);
        assert!(services.storage.run_next_job().await.unwrap());
    }
}

#[tokio::test]
async fn an_expired_attempt_cannot_keep_missing_audio_available_after_recovery() {
    use hito_core::Search;
    let (dir, services, actor, library, music) = support::library("server").await;
    let path = music.join("track.wav");
    support::write_wav(&path, 0);
    support::scan(&services, &actor, &library).await;
    let folder = services
        .storage
        .folders(&actor, &library)
        .await
        .unwrap()
        .remove(0);
    let job = folder.latest_job.unwrap();
    let pool = sqlx::SqlitePool::connect_with(
        sqlx::sqlite::SqliteConnectOptions::new().filename(dir.path().join("hito.db")),
    )
    .await
    .unwrap();
    sqlx::query("UPDATE jobs SET state = 'running', lease_until = 0, lease_token = 'interrupted-attempt' WHERE id = ?")
        .bind(&job.id).execute(&pool).await.unwrap();
    pool.close().await;
    std::fs::remove_file(path).unwrap();
    assert!(services.storage.run_next_job().await.unwrap());
    assert_eq!(
        services.storage.job(&actor, &job.id).await.unwrap().state,
        "complete"
    );
    let recordings = services
        .catalog
        .recordings(&actor, Search::default())
        .await
        .unwrap();
    assert_eq!(recordings.total, 1);
    assert!(!recordings.items[0].available);
    assert!(
        !services
            .catalog
            .sources(&actor, &recordings.items[0].id)
            .await
            .unwrap()[0]
            .ready
    );
}
