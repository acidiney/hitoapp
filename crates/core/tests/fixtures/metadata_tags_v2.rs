use sqlx::sqlite::SqliteConnectOptions;
use std::path::Path;

pub async fn represent_prior_inspection(database: &Path, source: &str) {
    let pool = sqlx::SqlitePool::connect_with(
        SqliteConnectOptions::new()
            .filename(database)
            .foreign_keys(true),
    )
    .await
    .unwrap();
    let mut transaction = pool.begin_with("BEGIN IMMEDIATE").await.unwrap();
    let observation = sqlx::query(
        "UPDATE metadata_observations
         SET inspection_version = 2, tags = json_remove(tags, '$.musicbrainz_artist_tags', '$.isrc')
         WHERE id = (SELECT observation_id FROM source_metadata WHERE source_id = ?)
           AND inspection_version = 4 AND origin = 'file_inspection'
           AND EXISTS (
               SELECT 1 FROM local_files
               WHERE source_id = ? AND inspection_scheme = 'ffmpeg-pcm-f64-v1'
                 AND length(audio_digest) = 64
           )",
    )
    .bind(source)
    .bind(source)
    .execute(&mut *transaction)
    .await
    .unwrap();
    assert_eq!(observation.rows_affected(), 1);
    let selected = sqlx::query(
        "UPDATE source_metadata SET inspection_version = 2
         WHERE source_id = ? AND inspection_version = 4",
    )
    .bind(source)
    .execute(&mut *transaction)
    .await
    .unwrap();
    assert_eq!(selected.rows_affected(), 1);
    transaction.commit().await.unwrap();
    pool.close().await;
}
