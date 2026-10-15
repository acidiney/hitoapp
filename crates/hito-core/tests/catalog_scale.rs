mod support;

use hito_core::Browse;
use sqlx::sqlite::SqliteConnectOptions;
use std::time::Duration;
use support::library;

#[tokio::test]
async fn compilation_heavy_catalog_reads_do_not_multiply_artists_by_every_album() {
    let (dir, hito, actor, library, _) = library("catalog-scale").await;
    let pool = sqlx::SqlitePool::connect_with(
        SqliteConnectOptions::new()
            .filename(dir.path().join("hito.db"))
            .foreign_keys(true),
    )
    .await
    .unwrap();
    let mut tx = pool.begin().await.unwrap();
    sqlx::query(
        "WITH RECURSIVE n(i) AS (VALUES(1) UNION ALL SELECT i + 1 FROM n WHERE i < 10000)
         INSERT INTO releases(id, library_id, title, artist, identity_key)
         SELECT 'album-' || i, ?, 'Album ' || i, 'Compilation credit', 'album-' || i FROM n",
    )
    .bind(&library)
    .execute(&mut *tx)
    .await
    .unwrap();
    sqlx::query(
        "WITH RECURSIVE n(i) AS (VALUES(0) UNION ALL SELECT i + 1 FROM n WHERE i < 99999)
         INSERT INTO recordings(id, library_id, release_id, title, artist, duration_ms, initial_file_checksum)
         SELECT 'track-' || i, ?, 'album-' || (1 + i / 10), 'Track ' || i,
             'Performer ' || (1 + i / 10), 1000, 'checksum-' || i FROM n",
    ).bind(&library).execute(&mut *tx).await.unwrap();
    sqlx::query("INSERT INTO sources(id, recording_id, provider, provider_id, ready) SELECT id, id, 'fixture', id, 0 FROM recordings")
        .execute(&mut *tx).await.unwrap();
    sqlx::query("INSERT INTO source_metadata(source_id, release_id, title, artist, duration_ms, track_number)
        SELECT id, release_id, title, artist, duration_ms, 1 FROM recordings")
        .execute(&mut *tx).await.unwrap();
    sqlx::query("INSERT INTO source_artist_credits(source_id, artist_id)
        SELECT m.source_id, c.id FROM source_metadata m JOIN sources s ON s.id = m.source_id
        JOIN recordings r ON r.id = s.recording_id JOIN artists c ON c.library_id = r.library_id AND c.name = m.artist COLLATE BINARY")
        .execute(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    for analyzed in [false, true] {
        if analyzed {
            sqlx::query("ANALYZE").execute(&pool).await.unwrap();
        }
        let page = tokio::time::timeout(
            Duration::from_secs(5),
            hito.catalog.artists(&actor, Browse::default()),
        )
        .await
        .expect("Compilation-heavy artist reads exceeded the smoke-test deadline")
        .unwrap();
        assert_eq!(page.total, 10001);
        assert_eq!(page.items.len(), 60);
        let compilation = page
            .items
            .iter()
            .find(|artist| artist.name == "Compilation credit")
            .unwrap();
        assert_eq!(compilation.release_count, 10000);
        let detail = tokio::time::timeout(
            Duration::from_secs(5),
            hito.catalog
                .artist(&actor, &compilation.id, Browse::default()),
        )
        .await
        .expect("Compilation detail exceeded the smoke-test deadline")
        .unwrap();
        assert_eq!(detail.releases.total, 10000);
        assert_eq!(detail.releases.items.len(), 60);
    }
}
