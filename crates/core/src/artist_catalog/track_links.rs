use super::{CatalogTrackMatch, MetadataProvider, authority::provider_columns};
use crate::{Actor, ArtistCatalog, Error, Result, catalog::readable_recordings_on};
use std::collections::HashSet;
use uuid::Uuid;

impl ArtistCatalog {
    pub(super) async fn link_catalog_track(
        &self,
        actor: &Actor,
        provider: &MetadataProvider,
        edition_id: &str,
        occurrence_id: &str,
        recording_id: &str,
    ) -> Result<()> {
        let (kind, installation) = provider_columns(provider);
        let mut connection = self.store.pool.acquire().await?;
        actor.check_session_on(&mut connection).await?;
        sqlx::query(
            "INSERT INTO catalog_track_links(user_id, provider_kind, plugin_installation_id, edition_id, occurrence_id, recording_id)
            VALUES(?, ?, ?, ?, ?, ?)
            ON CONFLICT(user_id, provider_kind, COALESCE(plugin_installation_id, ''), edition_id, occurrence_id)
            DO UPDATE SET recording_id = excluded.recording_id, updated_at = unixepoch()",
        )
        .bind(actor.id())
        .bind(kind)
        .bind(installation)
        .bind(edition_id)
        .bind(occurrence_id)
        .bind(recording_id)
        .execute(&mut *connection)
        .await?;
        Ok(())
    }

    pub(super) async fn linked_catalog_tracks(
        &self,
        actor: &Actor,
        provider: &MetadataProvider,
        edition_id: &str,
        only_occurrence: Option<&str>,
    ) -> Result<Vec<CatalogTrackMatch>> {
        let (kind, installation) = provider_columns(provider);
        let mut connection = self.store.pool.acquire().await?;
        actor.check_session_on(&mut connection).await?;
        let links: Vec<(String, String)> = sqlx::query_as(
            "SELECT link.occurrence_id, link.recording_id FROM catalog_track_links link
            JOIN recordings r ON r.id = link.recording_id
            JOIN library_members member ON member.library_id = r.library_id AND member.user_id = link.user_id
            WHERE link.user_id = ?1 AND link.provider_kind = ?2 AND link.plugin_installation_id IS ?3
                AND link.edition_id = ?4 AND (?5 IS NULL OR link.occurrence_id = ?5)
            ORDER BY link.occurrence_id",
        )
        .bind(actor.id())
        .bind(kind)
        .bind(installation)
        .bind(edition_id)
        .bind(only_occurrence)
        .fetch_all(&mut *connection)
        .await?;
        let mut distinct = HashSet::new();
        let ids = links
            .iter()
            .filter(|(_, recording)| distinct.insert(recording.as_str()))
            .map(|(_, recording)| Uuid::parse_str(recording))
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|_| Error::Internal("A stored recording identity is invalid".into()))?;
        let readable = readable_recordings_on(&mut connection, &self.host, actor, &ids).await?;
        Ok(links
            .into_iter()
            .filter_map(|(occurrence_id, recording_id)| {
                readable
                    .iter()
                    .find(|recording| recording.id == recording_id)
                    .map(|recording| CatalogTrackMatch {
                        occurrence_id,
                        recording: recording.clone(),
                    })
            })
            .collect())
    }
}
