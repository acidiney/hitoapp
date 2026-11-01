use super::{MetadataHistoryQuery, RecordingMetadata, TextMetadata, history};
use crate::{Actor, Catalog, Error, Result};
use sqlx::Row;

impl Catalog {
    #[tracing::instrument(level = "debug", skip_all, err(Debug), fields(account_id = actor.id()))]
    pub async fn recording_metadata(
        &self,
        actor: &Actor,
        recording: &str,
        page: MetadataHistoryQuery,
    ) -> Result<RecordingMetadata> {
        let mut tx = self.store.pool.begin().await?;
        let row = sqlx::query(
            "SELECT r.id, r.title AS inherited, COALESCE(p.override_title, r.title) AS title,
                p.override_title, p.title_operation_id AS operation_id, r.artist AS inherited_artist, COALESCE(p.override_artist, r.artist) AS artist, p.override_artist, p.artist_operation_id, r.metadata_observation_id
             FROM recordings r JOIN library_members m ON m.library_id = r.library_id AND m.user_id = ?
             LEFT JOIN personal_recording_metadata p ON p.recording_id = r.id AND p.user_id = m.user_id
             WHERE r.id = ?",
        ).bind(actor.id()).bind(recording).fetch_optional(&mut *tx).await?.ok_or(Error::NotFound)?;
        let recording_id: String = row.try_get("id")?;
        let title = TextMetadata {
            value: row.try_get("title")?,
            inherited: row.try_get("inherited")?,
            personal: row.try_get("override_title")?,
            operation_id: row.try_get("operation_id")?,
        };
        let artist = TextMetadata {
            value: row.try_get("artist")?,
            inherited: row.try_get("inherited_artist")?,
            personal: row.try_get("override_artist")?,
            operation_id: row.try_get("artist_operation_id")?,
        };
        let selected: Option<String> = row.try_get("metadata_observation_id")?;
        let selected_observation = if let Some(id) = selected {
            sqlx::query("SELECT * FROM metadata_observations WHERE id = ? AND recording_id = ?")
                .bind(id)
                .bind(&recording_id)
                .fetch_optional(&mut *tx)
                .await?
                .map(history::observation)
                .transpose()?
        } else {
            None
        };
        let credited_artists = super::read_credited_artists(&mut tx, actor, &recording_id).await?;
        let observations =
            history::page(&mut tx, history::Target::Recording(&recording_id), page).await?;
        tx.commit().await?;
        tracing::debug!(
            count = observations.items.len(),
            "Recording metadata loaded"
        );
        Ok(RecordingMetadata {
            recording_id,
            title,
            artist,
            selected_observation,
            credited_artists,
            observations,
        })
    }
}
