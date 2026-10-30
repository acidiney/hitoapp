use hito_core::test_support::support;
use uuid::Uuid;

#[tokio::test]
async fn ordered_lookup_stays_bounded_at_one_hundred_thousand_recordings_with_and_without_statistics()
 {
    let (directory, hito, actor, library, _) = support::library("queue-scale").await;
    let pool = sqlx::SqlitePool::connect_with(
        sqlx::sqlite::SqliteConnectOptions::new().filename(directory.path().join("hito.db")),
    )
    .await
    .unwrap();
    sqlx::query("WITH RECURSIVE n(i) AS (VALUES(0) UNION ALL SELECT i + 1 FROM n WHERE i < 99999)
        INSERT INTO recordings(id,library_id,title,artist,duration_ms,initial_file_checksum)
        SELECT printf('00000000-0000-0000-0000-%012x', i), ?, 'Track ' || i, 'Fixture artist', 1000, 'checksum-' || i FROM n")
        .bind(&library).execute(&pool).await.unwrap();
    let mut requested: Vec<Uuid> = (0..1000)
        .map(|n| Uuid::from_u128((n * 97 % 100000) as u128))
        .collect();
    requested[999] = requested[0];
    for analyzed in [false, true] {
        if analyzed {
            sqlx::query("ANALYZE").execute(&pool).await.unwrap();
        }
        let selected = hito
            .catalog
            .recordings_by_ids(&actor, &requested)
            .await
            .unwrap();
        assert_eq!(
            selected
                .iter()
                .map(|recording| recording.id.clone())
                .collect::<Vec<_>>(),
            requested.iter().map(Uuid::to_string).collect::<Vec<_>>()
        );
        assert!(selected.iter().all(|recording| !recording.available));
    }
    pool.close().await;
}
