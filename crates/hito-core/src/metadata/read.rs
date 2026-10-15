use super::{
    MetadataHistoryQuery, MetadataObservation, ObservationOrigin, ObservationPage,
    RecordingMetadata, TitleMetadata,
};
use crate::{Actor, Catalog, Error, Result};
use sqlx::{Row, sqlite::SqliteRow};

impl Catalog {
    #[tracing::instrument(level = "debug", skip_all, err(Debug), fields(account_id = actor.id()))]
    pub async fn recording_metadata(
        &self,
        actor: &Actor,
        recording: &str,
        page: MetadataHistoryQuery,
    ) -> Result<RecordingMetadata> {
        if page.before.is_some_and(|before| before <= 0) {
            return Err(Error::Invalid("Use a positive observation cursor".into()));
        }
        let limit = page.limit.unwrap_or(20).clamp(1, 100);
        let mut tx = self.store.pool.begin().await?;
        let row = sqlx::query(
            "SELECT r.id, r.title AS inherited, COALESCE(p.override_title, r.title) AS title,
                p.override_title, p.title_operation_id AS operation_id, r.metadata_observation_id
             FROM recordings r JOIN library_members m ON m.library_id = r.library_id AND m.user_id = ?
             LEFT JOIN personal_recording_metadata p ON p.recording_id = r.id AND p.user_id = m.user_id
             WHERE r.id = ?",
        ).bind(actor.id()).bind(recording).fetch_optional(&mut *tx).await?.ok_or(Error::NotFound)?;
        let recording_id: String = row.try_get("id")?;
        let title = TitleMetadata {
            value: row.try_get("title")?,
            inherited: row.try_get("inherited")?,
            personal: row.try_get("override_title")?,
            operation_id: row.try_get("operation_id")?,
        };
        let selected: Option<String> = row.try_get("metadata_observation_id")?;
        let selected_observation = if let Some(id) = selected {
            sqlx::query("SELECT * FROM metadata_observations WHERE id = ? AND recording_id = ?")
                .bind(id)
                .bind(&recording_id)
                .fetch_optional(&mut *tx)
                .await?
                .map(observation)
                .transpose()?
        } else {
            None
        };
        let mut rows = sqlx::query(
            "SELECT * FROM metadata_observations WHERE recording_id = ? AND sequence < ? ORDER BY sequence DESC LIMIT ?",
        ).bind(&recording_id).bind(page.before.unwrap_or(i64::MAX)).bind(limit + 1).fetch_all(&mut *tx).await?;
        let next_before = if rows.len() > limit as usize {
            rows.pop();
            rows.last()
                .map(|row| row.try_get::<i64, _>("sequence"))
                .transpose()?
        } else {
            None
        };
        let items = rows
            .into_iter()
            .map(observation)
            .collect::<Result<Vec<_>>>()?;
        tx.commit().await?;
        tracing::debug!(count = items.len(), "Recording metadata loaded");
        Ok(RecordingMetadata {
            recording_id,
            title,
            selected_observation,
            observations: ObservationPage { items, next_before },
        })
    }
}

fn observation(row: SqliteRow) -> Result<MetadataObservation> {
    let origin: &str = row.try_get("origin")?;
    let origin = match origin {
        "file_inspection" => ObservationOrigin::FileInspection,
        "legacy_normalized" => ObservationOrigin::LegacyNormalized,
        _ => return Err(Error::Internal("Metadata origin could not be read".into())),
    };
    let tags: Option<String> = row.try_get("tags")?;
    let tags = tags
        .as_deref()
        .map(serde_json::from_str)
        .transpose()
        .map_err(|_| Error::Internal("Observed tags could not be read".into()))?;
    Ok(MetadataObservation {
        id: row.try_get("id")?,
        source_id: row.try_get("source_id")?,
        release_id: row.try_get("release_id")?,
        provider: row.try_get("provider")?,
        origin,
        observed_at: row.try_get("observed_at")?,
        inspection_version: row.try_get("inspection_version")?,
        title: row.try_get("title")?,
        artist: row.try_get("artist")?,
        album: row.try_get("album")?,
        album_artist: row.try_get("album_artist")?,
        duration_ms: row.try_get("duration_ms")?,
        disc_number: row.try_get("disc_number")?,
        track_number: row.try_get("track_number")?,
        tags,
    })
}
