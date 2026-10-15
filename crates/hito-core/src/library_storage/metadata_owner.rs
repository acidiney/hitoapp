use crate::Result;
use sqlx::{Sqlite, Transaction};

#[derive(sqlx::FromRow)]
struct Metadata {
    source: String,
    observation_id: Option<String>,
    release_id: Option<String>,
    title: String,
    artist: String,
    duration_ms: i64,
}

pub(super) async fn refresh_metadata(
    tx: &mut Transaction<'_, Sqlite>,
    recording: &str,
) -> Result<()> {
    let current: Option<Metadata> = sqlx::query_as(
        "SELECT s.id AS source, m.observation_id, m.release_id, m.title, m.artist, m.duration_ms
         FROM recordings r JOIN sources s ON s.id = r.metadata_source_id
         JOIN source_metadata m ON m.source_id = s.id JOIN local_files f ON f.source_id = s.id JOIN music_folders folder ON folder.id = f.folder_id
         WHERE r.id = ? AND s.recording_id = r.id AND f.library_id = r.library_id AND s.ready = 1 AND folder.connected = 1 AND folder.bound = 1",
    )
    .bind(recording)
    .fetch_optional(&mut **tx)
    .await?;
    let metadata = if current.is_some() {
        current
    } else {
        sqlx::query_as(
            "SELECT s.id AS source, m.observation_id, m.release_id, m.title, m.artist, m.duration_ms
             FROM sources s JOIN source_metadata m ON m.source_id = s.id
             JOIN local_files f ON f.source_id = s.id JOIN music_folders folder ON folder.id = f.folder_id JOIN recordings r ON r.id = s.recording_id
             WHERE r.id = ? AND f.library_id = r.library_id AND s.ready = 1 AND folder.connected = 1 AND folder.bound = 1 ORDER BY s.id LIMIT 1",
        )
        .bind(recording)
        .fetch_optional(&mut **tx)
        .await?
    };
    let Some(metadata) = metadata else {
        sqlx::query("UPDATE recordings SET metadata_source_id = NULL WHERE id = ? AND metadata_source_id IS NOT NULL")
            .bind(recording).execute(&mut **tx).await?;
        return Ok(());
    };
    sqlx::query(
        "UPDATE recordings SET metadata_source_id = ?, metadata_observation_id = ?, release_id = ?, title = ?, artist = ?, duration_ms = ?
         WHERE id = ? AND (metadata_source_id IS NOT ? OR metadata_observation_id IS NOT ? OR release_id IS NOT ? OR title != ? OR artist != ? OR duration_ms != ?)",
    ).bind(&metadata.source).bind(&metadata.observation_id).bind(&metadata.release_id).bind(&metadata.title).bind(&metadata.artist).bind(metadata.duration_ms)
        .bind(recording).bind(&metadata.source).bind(&metadata.observation_id).bind(&metadata.release_id).bind(&metadata.title).bind(&metadata.artist).bind(metadata.duration_ms)
        .execute(&mut **tx).await?;
    Ok(())
}

pub(super) async fn refresh_folder_metadata(
    tx: &mut Transaction<'_, Sqlite>,
    folder: &str,
) -> Result<()> {
    sqlx::query(
        "WITH affected AS MATERIALIZED (
           SELECT DISTINCT s.recording_id FROM local_files f JOIN sources s ON s.id = f.source_id WHERE f.folder_id = ?
         ), owners AS MATERIALIZED (
           SELECT r.id, CASE WHEN EXISTS(
             SELECT 1 FROM sources s JOIN local_files f ON f.source_id = s.id JOIN music_folders m ON m.id = f.folder_id
             WHERE s.id = r.metadata_source_id AND s.ready = 1 AND m.connected = 1 AND m.bound = 1
           ) THEN r.metadata_source_id ELSE (
             SELECT s.id FROM sources s JOIN local_files f ON f.source_id = s.id JOIN music_folders m ON m.id = f.folder_id
             WHERE s.recording_id = r.id AND s.ready = 1 AND m.connected = 1 AND m.bound = 1 ORDER BY s.id LIMIT 1
           ) END AS source FROM recordings r JOIN affected a ON a.recording_id = r.id
         ) UPDATE recordings AS r SET
           metadata_source_id = o.source,
           metadata_observation_id = CASE WHEN o.source IS NULL THEN r.metadata_observation_id ELSE m.observation_id END,
           release_id = CASE WHEN o.source IS NULL THEN r.release_id ELSE m.release_id END,
           title = COALESCE(m.title, r.title), artist = COALESCE(m.artist, r.artist), duration_ms = COALESCE(m.duration_ms, r.duration_ms)
         FROM owners o LEFT JOIN source_metadata m ON m.source_id = o.source
         WHERE r.id = o.id AND (r.metadata_source_id IS NOT o.source OR (o.source IS NOT NULL AND r.metadata_observation_id IS NOT m.observation_id) OR r.title IS NOT COALESCE(m.title, r.title)
           OR r.artist IS NOT COALESCE(m.artist, r.artist) OR r.duration_ms IS NOT COALESCE(m.duration_ms, r.duration_ms)
           OR (o.source IS NOT NULL AND r.release_id IS NOT m.release_id))",
    ).bind(folder).execute(&mut **tx).await?;
    Ok(())
}
