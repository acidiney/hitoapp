use super::{Artwork, ArtworkAvailability, Catalog, assets, jobs};
use crate::{Actor, Error, Result};
use serde::Serialize;
use sqlx::SqliteConnection;
use utoipa::ToSchema;
use uuid::Uuid;

#[derive(Serialize, ToSchema, sqlx::FromRow)]
pub struct PlaylistArtworkFetch {
    pub id: String,
    pub playlist_id: String,
    #[sqlx(try_from = "String")]
    pub status: jobs::ArtworkFetchStatus,
    #[sqlx(try_from = "i64")]
    pub attempts: u32,
    pub error: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(sqlx::FromRow)]
struct ApprovedPlaylistProvider {
    installation_id: String,
    installation_revision: i64,
    service_id: String,
    service_revision: i64,
    provider: String,
    provider_playlist_id: String,
    snapshot: String,
    imported_at: i64,
}

impl Catalog {
    #[tracing::instrument(level = "debug", skip_all, err(Debug), fields(account_id = actor.id()))]
    pub async fn playlist_artwork_availability(
        &self,
        actor: &Actor,
        playlist: &str,
    ) -> Result<ArtworkAvailability> {
        let started = std::time::Instant::now();
        let mut tx = self.store.pool.begin().await?;
        actor.check_session_on(&mut tx).await?;
        access_on(&mut tx, actor, playlist).await?;
        let can_fetch = approved_provider(&mut tx, actor.id(), playlist)
            .await?
            .is_some();
        tx.commit().await?;
        actor.check_session(&self.store).await?;
        tracing::debug!(
            can_fetch,
            elapsed_ms = started.elapsed().as_millis() as u64,
            "Playlist artwork availability read"
        );
        Ok(ArtworkAvailability { can_fetch })
    }

    #[tracing::instrument(level = "info", skip_all, err(Debug), fields(account_id = actor.id()))]
    pub async fn enqueue_playlist_artwork(
        &self,
        actor: &Actor,
        playlist: &str,
    ) -> Result<PlaylistArtworkFetch> {
        let started = std::time::Instant::now();
        let mut tx = self.store.pool.begin_with("BEGIN IMMEDIATE").await?;
        actor.check_session_on(&mut tx).await?;
        access_on(&mut tx, actor, playlist).await?;
        jobs::maintain(&mut tx).await?;
        let pending: Option<(String, String)> = sqlx::query_as(
            "SELECT id,session_hash FROM artwork_fetches WHERE user_id = ? AND playlist_id = ?
             AND entity_kind = 'playlist' AND status IN ('queued','running')",
        )
        .bind(actor.id())
        .bind(playlist)
        .fetch_optional(&mut *tx)
        .await?;
        if let Some((id, session)) = pending {
            if session != actor.session_binding() {
                return Err(Error::Conflict(
                    "Artwork is already being retrieved by another session".into(),
                ));
            }
            let fetch = read_on(&mut tx, actor.id(), &id).await?;
            tx.commit().await?;
            actor.check_session(&self.store).await?;
            tracing::info!(job_id = %id, outcome = "already_pending", elapsed_ms = started.elapsed().as_millis() as u64, "Playlist artwork request accepted");
            return Ok(fetch);
        }
        let previous: Option<String> = sqlx::query_scalar(
            "SELECT j.id FROM playlist_artwork retained JOIN artwork_fetches j ON j.id = retained.fetch_id
             JOIN playlist_imports origin ON origin.playlist_id = retained.playlist_id
                AND origin.installation_id = j.installation_id AND origin.service_id = j.service_id
                AND origin.provider = j.playlist_provider AND origin.provider_playlist_id = j.provider_playlist_id
                AND origin.snapshot = j.playlist_snapshot AND origin.imported_at = j.playlist_imported_at
             WHERE retained.user_id = ? AND retained.playlist_id = ? AND j.status = 'ready' LIMIT 1",
        ).bind(actor.id()).bind(playlist).fetch_optional(&mut *tx).await?;
        if let Some(id) = previous {
            let fetch = read_on(&mut tx, actor.id(), &id).await?;
            tx.commit().await?;
            actor.check_session(&self.store).await?;
            tracing::info!(job_id = %id, outcome = "already_retained", elapsed_ms = started.elapsed().as_millis() as u64, "Playlist artwork request accepted");
            return Ok(fetch);
        }
        jobs::capacity(&mut tx, actor.id()).await?;
        let provider = approved_provider(&mut tx, actor.id(), playlist)
            .await?
            .ok_or_else(|| {
                Error::Invalid("This playlist has no approved artwork provider".into())
            })?;
        let id = Uuid::new_v4().to_string();
        sqlx::query(
            "INSERT INTO artwork_fetches(id,user_id,session_hash,installation_id,installation_revision,
                service_id,service_revision,status,reservation_bytes,entity_kind,playlist_id,provider_playlist_id,
                playlist_provider,playlist_snapshot,playlist_imported_at)
             VALUES (?,?,?,?,?,?,?,'queued',?,'playlist',?,?,?,?,?)",
        ).bind(&id).bind(actor.id()).bind(actor.session_binding())
            .bind(provider.installation_id).bind(provider.installation_revision)
            .bind(provider.service_id).bind(provider.service_revision).bind(jobs::RESERVATION_BYTES)
            .bind(playlist).bind(provider.provider_playlist_id).bind(provider.provider).bind(provider.snapshot)
            .bind(provider.imported_at).execute(&mut *tx).await?;
        let fetch = read_on(&mut tx, actor.id(), &id).await?;
        tx.commit().await?;
        actor.check_session(&self.store).await?;
        tracing::info!(job_id = %id, outcome = "queued", elapsed_ms = started.elapsed().as_millis() as u64, "Playlist artwork request accepted");
        Ok(fetch)
    }

    #[tracing::instrument(level = "debug", skip_all, err(Debug), fields(account_id = actor.id()))]
    pub async fn playlist_artwork_fetch(
        &self,
        actor: &Actor,
        id: &str,
    ) -> Result<PlaylistArtworkFetch> {
        let started = std::time::Instant::now();
        let mut tx = self.store.pool.begin().await?;
        actor.check_session_on(&mut tx).await?;
        let mut fetch = read_on(&mut tx, actor.id(), id).await?;
        access_on(&mut tx, actor, &fetch.playlist_id).await?;
        (fetch.status, fetch.error) = jobs::observed_status(
            &mut tx,
            actor.id(),
            id,
            fetch.status,
            fetch.attempts,
            fetch.error,
        )
        .await?;
        tx.commit().await?;
        actor.check_session(&self.store).await?;
        tracing::debug!(job_id = %fetch.id, status = ?fetch.status,
            elapsed_ms = started.elapsed().as_millis() as u64, "Playlist artwork request read");
        Ok(fetch)
    }

    #[tracing::instrument(level = "debug", skip_all, err(Debug), fields(account_id = actor.id()))]
    pub async fn playlist_artwork(&self, actor: &Actor, playlist: &str) -> Result<Option<Artwork>> {
        let started = std::time::Instant::now();
        let mut tx = self.store.pool.begin().await?;
        actor.check_session_on(&mut tx).await?;
        access_on(&mut tx, actor, playlist).await?;
        let row: Option<(String, Vec<u8>)> = sqlx::query_as(
            "SELECT asset.content_type,asset.bytes FROM playlist_artwork retained
             JOIN artwork_assets asset ON asset.user_id = retained.user_id AND asset.content_hash = retained.content_hash
             JOIN playlists p ON p.id = retained.playlist_id AND p.user_id = retained.user_id AND p.deleted = 0
             WHERE retained.user_id = ? AND retained.playlist_id = ?",
        ).bind(actor.id()).bind(playlist).fetch_optional(&mut *tx).await?;
        tx.commit().await?;
        actor.check_session(&self.store).await?;
        tracing::debug!(
            found = row.is_some(),
            elapsed_ms = started.elapsed().as_millis() as u64,
            "Retained playlist artwork read"
        );
        row.map(assets::stored).transpose()
    }
}

async fn approved_provider(
    connection: &mut SqliteConnection,
    account: &str,
    playlist: &str,
) -> Result<Option<ApprovedPlaylistProvider>> {
    Ok(sqlx::query_as(
        "SELECT installation.id AS installation_id,installation.revision AS installation_revision,
            service.id AS service_id,service.revision AS service_revision,origin.provider,
            origin.provider_playlist_id,origin.snapshot,origin.imported_at
         FROM playlist_imports origin JOIN playlists p ON p.id = origin.playlist_id
            AND p.user_id = ? AND p.deleted = 0 AND p.kind = 'local'
         JOIN plugin_installations installation ON installation.id = origin.installation_id
            AND installation.user_id = p.user_id AND installation.active = 1
            AND installation.service_id = origin.service_id AND json_extract(installation.manifest,'$.id') = origin.provider
         JOIN plugin_services service ON service.id = origin.service_id AND service.active = 1
         WHERE p.id = ? AND EXISTS(SELECT 1 FROM json_each(installation.manifest,'$.capabilities') WHERE value = 'artwork')",
    ).bind(account).bind(playlist).fetch_optional(connection).await?)
}

async fn read_on(
    connection: &mut SqliteConnection,
    account: &str,
    id: &str,
) -> Result<PlaylistArtworkFetch> {
    sqlx::query_as(
        "SELECT id,playlist_id,status,attempts,error,created_at,updated_at FROM artwork_fetches
         WHERE id = ? AND user_id = ? AND entity_kind = 'playlist'",
    )
    .bind(id)
    .bind(account)
    .fetch_optional(connection)
    .await?
    .ok_or(Error::NotFound)
}

async fn access_on(connection: &mut SqliteConnection, actor: &Actor, playlist: &str) -> Result<()> {
    let allowed: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM playlists WHERE id = ? AND user_id = ? AND deleted = 0)",
    )
    .bind(playlist)
    .bind(actor.id())
    .fetch_one(connection)
    .await?;
    if allowed {
        Ok(())
    } else {
        Err(Error::NotFound)
    }
}
