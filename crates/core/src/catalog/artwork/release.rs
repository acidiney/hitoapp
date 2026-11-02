use super::{Artwork, Catalog, assets, availability, jobs};
use crate::{Actor, Error, Result};
use jobs::EntityKind;
use serde::Serialize;
use sqlx::SqliteConnection;
use utoipa::ToSchema;
use uuid::Uuid;

#[derive(Serialize, ToSchema)]
pub struct ReleaseArtworkFetch {
    pub id: String,
    pub release_id: String,
    pub status: jobs::ArtworkFetchStatus,
    pub attempts: u32,
    pub error: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

impl From<jobs::FetchRow> for ReleaseArtworkFetch {
    fn from(row: jobs::FetchRow) -> Self {
        Self {
            id: row.id,
            release_id: row.entity_id,
            status: row.status,
            attempts: row.attempts,
            error: row.error,
            created_at: row.created_at,
            updated_at: row.updated_at,
        }
    }
}

impl Catalog {
    #[tracing::instrument(level = "info", skip_all, err(Debug), fields(account_id = actor.id()))]
    pub async fn enqueue_release_artwork(
        &self,
        actor: &Actor,
        release: &str,
    ) -> Result<ReleaseArtworkFetch> {
        let started = std::time::Instant::now();
        let mut tx = self.store.pool.begin_with("BEGIN IMMEDIATE").await?;
        actor.check_session_on(&mut tx).await?;
        access_on(&mut tx, actor, release).await?;
        jobs::maintain(&mut tx).await?;
        let pending: Option<(String, String)> = sqlx::query_as(
            "SELECT id,session_hash FROM artwork_fetches WHERE user_id = ? AND release_id = ?
             AND entity_kind = 'release' AND status IN ('queued','running')",
        )
        .bind(actor.id())
        .bind(release)
        .fetch_optional(&mut *tx)
        .await?;
        if let Some((id, session)) = pending {
            if session != actor.session_binding() {
                return Err(Error::Conflict(
                    "Artwork is already being retrieved by another session".into(),
                ));
            }
            let fetch: ReleaseArtworkFetch =
                jobs::fetch_row(&mut tx, actor.id(), &id, EntityKind::Release)
                    .await?
                    .into();
            tx.commit().await?;
            actor.check_session(&self.store).await?;
            tracing::info!(job_id = %id, outcome = "already_pending", elapsed_ms = started.elapsed().as_millis() as u64, "Album artwork request accepted");
            return Ok(fetch);
        }
        let previous: Option<String> = sqlx::query_scalar(
            "SELECT j.id FROM release_artwork a JOIN artwork_fetches j ON j.id = a.fetch_id
             JOIN recordings r ON r.id = j.recording_id AND r.metadata_observation_id = a.observation_id
                AND r.release_id = a.release_id
             WHERE a.user_id = ? AND a.release_id = ? AND j.status = 'ready' LIMIT 1",
        )
        .bind(actor.id())
        .bind(release)
        .fetch_optional(&mut *tx)
        .await?;
        if let Some(id) = previous {
            let fetch: ReleaseArtworkFetch =
                jobs::fetch_row(&mut tx, actor.id(), &id, EntityKind::Release)
                    .await?
                    .into();
            tx.commit().await?;
            actor.check_session(&self.store).await?;
            tracing::info!(job_id = %id, outcome = "already_retained", elapsed_ms = started.elapsed().as_millis() as u64, "Album artwork request accepted");
            return Ok(fetch);
        }
        jobs::capacity(&mut tx, actor.id()).await?;
        let provider = availability::approved_release_provider(&mut tx, actor.id(), release)
            .await?
            .ok_or_else(|| Error::Invalid("This album has no approved artwork provider".into()))?;
        let provider_release = provider.provider_release_id.ok_or_else(|| {
            Error::Internal("The album artwork provider has no release witness".into())
        })?;
        let id = Uuid::new_v4().to_string();
        sqlx::query(
            "INSERT INTO artwork_fetches(id,user_id,recording_id,session_hash,observation_id,binding_id,binding_revision,
                installation_id,installation_revision,service_id,service_revision,status,reservation_bytes,
                entity_kind,release_id,provider_release_id)
             VALUES (?,?,?,?,?,?,?,?,?,?,?,'queued',?,'release',?,?)",
        )
        .bind(&id).bind(actor.id()).bind(provider.recording_id).bind(actor.session_binding())
        .bind(provider.observation_id).bind(provider.binding_id).bind(provider.binding_revision)
        .bind(provider.installation_id).bind(provider.installation_revision)
        .bind(provider.service_id).bind(provider.service_revision).bind(jobs::RESERVATION_BYTES)
        .bind(release).bind(provider_release)
        .execute(&mut *tx).await?;
        actor.check_session_on(&mut tx).await?;
        let fetch: ReleaseArtworkFetch =
            jobs::fetch_row(&mut tx, actor.id(), &id, EntityKind::Release)
                .await?
                .into();
        tx.commit().await?;
        actor.check_session(&self.store).await?;
        tracing::info!(job_id = %id, outcome = "queued", elapsed_ms = started.elapsed().as_millis() as u64, "Album artwork request accepted");
        Ok(fetch)
    }

    #[tracing::instrument(level = "debug", skip_all, err(Debug), fields(account_id = actor.id()))]
    pub async fn release_artwork_fetch(
        &self,
        actor: &Actor,
        id: &str,
    ) -> Result<ReleaseArtworkFetch> {
        let started = std::time::Instant::now();
        let fetch: ReleaseArtworkFetch = self
            .read_artwork_fetch(actor, id, EntityKind::Release)
            .await?
            .into();
        tracing::debug!(job_id = %fetch.id, status = ?fetch.status,
            elapsed_ms = started.elapsed().as_millis() as u64, "Album artwork request read");
        Ok(fetch)
    }

    pub(super) async fn queue_provider_release_artwork(
        &self,
        actor: &Actor,
        release: &str,
    ) -> Result<()> {
        let mut tx = self.store.pool.begin().await?;
        actor.check_session_on(&mut tx).await?;
        let wanted = availability::automatic_artwork_allowed(&mut tx, actor.id()).await?
            && !sqlx::query_scalar::<_, bool>(
                "SELECT EXISTS(SELECT 1 FROM artwork_fetches WHERE user_id = ? AND release_id = ?
                 AND entity_kind = 'release' AND status IN ('queued','running','failed'))",
            )
            .bind(actor.id())
            .bind(release)
            .fetch_one(&mut *tx)
            .await?
            && availability::approved_release_provider(&mut tx, actor.id(), release)
                .await?
                .is_some();
        tx.commit().await?;
        if !wanted {
            return Ok(());
        }
        match self.enqueue_release_artwork(actor, release).await {
            Ok(_) | Err(Error::Conflict(_) | Error::Invalid(_)) => Ok(()),
            Err(error) => Err(error),
        }
    }

    pub(super) async fn retained_release_artwork(
        &self,
        actor: &Actor,
        release: &str,
    ) -> Result<Option<Artwork>> {
        let mut tx = self.store.pool.begin().await?;
        actor.check_session_on(&mut tx).await?;
        access_on(&mut tx, actor, release).await?;
        let row: Option<(String, Vec<u8>)> = sqlx::query_as(
            "SELECT asset.content_type,asset.bytes FROM release_artwork retained
             JOIN artwork_assets asset ON asset.user_id = retained.user_id AND asset.content_hash = retained.content_hash
             JOIN releases a ON a.id = retained.release_id
             JOIN libraries l ON l.id = a.library_id AND l.owner_id = retained.user_id
             WHERE retained.user_id = ? AND retained.release_id = ?",
        ).bind(actor.id()).bind(release).fetch_optional(&mut *tx).await?;
        tx.commit().await?;
        actor.check_session(&self.store).await?;
        tracing::debug!(found = row.is_some(), "Retained album artwork read");
        row.map(assets::stored).transpose()
    }
}

pub(super) async fn access_on(
    connection: &mut SqliteConnection,
    actor: &Actor,
    release: &str,
) -> Result<()> {
    let allowed: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM releases a JOIN library_members member ON member.library_id = a.library_id
         WHERE a.id = ? AND member.user_id = ?)",
    )
    .bind(release)
    .bind(actor.id())
    .fetch_one(connection)
    .await?;
    if allowed {
        Ok(())
    } else {
        Err(Error::NotFound)
    }
}
