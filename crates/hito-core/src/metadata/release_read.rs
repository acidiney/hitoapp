use super::{MetadataHistoryQuery, ReleaseMetadata, TextMetadata, history};
use crate::{Actor, Catalog, Error, Result};
use sqlx::Row;

impl Catalog {
    #[tracing::instrument(level = "debug", skip_all, err(Debug), fields(account_id = actor.id()))]
    pub async fn release_metadata(
        &self,
        actor: &Actor,
        release: &str,
        page: MetadataHistoryQuery,
    ) -> Result<ReleaseMetadata> {
        let mut tx = self.store.pool.begin().await?;
        let row = sqlx::query("SELECT a.id, a.title AS inherited_title, a.artist AS inherited_artist,
            COALESCE(p.override_title, a.title) AS title, COALESCE(p.override_artist, a.artist) AS artist,
            p.override_title, p.override_artist, p.title_operation_id, p.artist_operation_id
            FROM releases a JOIN library_members m ON m.library_id = a.library_id AND m.user_id = ?
            LEFT JOIN personal_release_metadata p ON p.user_id = m.user_id AND p.release_id = a.id WHERE a.id = ?")
            .bind(actor.id()).bind(release).fetch_optional(&mut *tx).await?.ok_or(Error::NotFound)?;
        let release_id: String = row.try_get("id")?;
        let title = TextMetadata {
            value: row.try_get("title")?,
            inherited: row.try_get("inherited_title")?,
            personal: row.try_get("override_title")?,
            operation_id: row.try_get("title_operation_id")?,
        };
        let artist = TextMetadata {
            value: row.try_get("artist")?,
            inherited: row.try_get("inherited_artist")?,
            personal: row.try_get("override_artist")?,
            operation_id: row.try_get("artist_operation_id")?,
        };
        let observations =
            history::page(&mut tx, history::Target::Release(&release_id), page).await?;
        tx.commit().await?;
        tracing::debug!(count = observations.items.len(), "Release metadata loaded");
        Ok(ReleaseMetadata {
            release_id,
            title,
            artist,
            observations,
        })
    }
}
