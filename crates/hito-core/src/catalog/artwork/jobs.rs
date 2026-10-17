use super::Catalog;
use crate::{Actor, Error, Result, identity::recording_access};
use serde::Serialize;
use sqlx::SqliteConnection;
use utoipa::ToSchema;
use uuid::Uuid;

pub(super) const RESERVATION_BYTES: i64 = 2 * 1024 * 1024;

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ArtworkFetchStatus {
    Queued,
    Running,
    Ready,
    Failed,
    Cancelled,
}

#[derive(Serialize, ToSchema)]
pub struct ArtworkFetch {
    pub id: String,
    pub recording_id: String,
    pub status: ArtworkFetchStatus,
    pub attempts: u32,
    pub error: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

impl Catalog {
    #[tracing::instrument(level = "info", skip_all, err(Debug), fields(account_id = actor.id()))]
    pub async fn enqueue_recording_artwork(
        &self,
        actor: &Actor,
        recording: &str,
    ) -> Result<ArtworkFetch> {
        let started = std::time::Instant::now();
        let mut tx = self.store.pool.begin_with("BEGIN IMMEDIATE").await?;
        actor.check_session_on(&mut tx).await?;
        recording_access(&mut tx, actor, recording).await?;
        maintain(&mut tx).await?;
        let pending: Option<(String, String)> = sqlx::query_as(
            "SELECT id,session_hash FROM artwork_fetches WHERE user_id = ? AND recording_id = ?
             AND status IN ('queued','running')",
        )
        .bind(actor.id())
        .bind(recording)
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
            tracing::info!(job_id = %id, outcome = "already_pending", elapsed_ms = started.elapsed().as_millis() as u64, "Artwork request accepted");
            return Ok(fetch);
        }
        let previous: Option<String> = sqlx::query_scalar(
            "SELECT j.id FROM recording_artwork a JOIN artwork_fetches j ON j.id = a.fetch_id
             JOIN recordings r ON r.id = a.recording_id AND r.metadata_observation_id = a.observation_id
             WHERE j.user_id = ? AND j.recording_id = ? AND j.status = 'ready'
             LIMIT 1",
        ).bind(actor.id()).bind(recording).fetch_optional(&mut *tx).await?;
        if let Some(id) = previous {
            let fetch = read_on(&mut tx, actor.id(), &id).await?;
            tx.commit().await?;
            tracing::info!(job_id = %id, outcome = "already_retained", elapsed_ms = started.elapsed().as_millis() as u64, "Artwork request accepted");
            return Ok(fetch);
        }
        capacity(&mut tx, actor.id()).await?;
        let id = Uuid::new_v4().to_string();
        let inserted = sqlx::query(
            "INSERT INTO artwork_fetches(id,user_id,recording_id,session_hash,observation_id,binding_id,binding_revision,
                installation_id,installation_revision,service_id,service_revision,status,reservation_bytes)
             SELECT ?,?,r.id,?,o.id,binding.id,binding.revision,p.id,p.revision,service.id,service.revision,'queued',?
             FROM recordings r JOIN libraries l ON l.id = r.library_id AND l.owner_id = ?
             JOIN metadata_observations o ON o.id = r.metadata_observation_id AND o.recording_id = r.id
                AND o.origin = 'provider_response'
             JOIN provider_items binding ON binding.id = r.metadata_binding_id AND binding.id = o.provider_binding_id
                AND binding.recording_id = r.id AND binding.retired = 0
                AND binding.installation_id = o.plugin_installation_id AND binding.provider_recording_id = o.provider_recording_id
             JOIN plugin_installations p ON p.id = binding.installation_id AND p.user_id = ? AND p.active = 1
                AND json_extract(p.manifest,'$.id') = o.provider
             JOIN plugin_services service ON service.id = p.service_id AND service.active = 1
             WHERE r.id = ? AND EXISTS(SELECT 1 FROM json_each(p.manifest,'$.capabilities') WHERE value = 'artwork')",
        ).bind(&id).bind(actor.id()).bind(actor.session_binding()).bind(RESERVATION_BYTES)
            .bind(actor.id()).bind(actor.id()).bind(recording).execute(&mut *tx).await?.rows_affected();
        if inserted != 1 {
            return Err(Error::Invalid(
                "This recording has no approved artwork provider".into(),
            ));
        }
        actor.check_session_on(&mut tx).await?;
        let fetch = read_on(&mut tx, actor.id(), &id).await?;
        tx.commit().await?;
        tracing::info!(job_id = %id, outcome = "queued", elapsed_ms = started.elapsed().as_millis() as u64, "Artwork request accepted");
        Ok(fetch)
    }

    #[tracing::instrument(level = "debug", skip_all, err(Debug), fields(account_id = actor.id()))]
    pub async fn artwork_fetch(&self, actor: &Actor, id: &str) -> Result<ArtworkFetch> {
        let mut tx = self.store.pool.begin().await?;
        actor.check_session_on(&mut tx).await?;
        let mut fetch = read_on(&mut tx, actor.id(), id).await?;
        recording_access(&mut tx, actor, &fetch.recording_id).await?;
        if matches!(
            &fetch.status,
            ArtworkFetchStatus::Queued | ArtworkFetchStatus::Running
        ) {
            let (current, lease_expired): (bool, bool) = sqlx::query_as(
                "SELECT EXISTS(SELECT 1 FROM current_artwork_fetch_authority authority WHERE authority.id = j.id),
                    coalesce(j.lease_until <= unixepoch(),0)
                 FROM artwork_fetches j WHERE j.id = ? AND j.user_id = ?",
            ).bind(id).bind(actor.id()).fetch_one(&mut *tx).await?;
            if !current {
                fetch.status = ArtworkFetchStatus::Cancelled;
                fetch.error = Some("authority_changed".into());
            } else if lease_expired {
                (fetch.status, fetch.error) = if fetch.attempts < 3 {
                    (ArtworkFetchStatus::Queued, None)
                } else {
                    (
                        ArtworkFetchStatus::Failed,
                        Some("attempts_exhausted".into()),
                    )
                };
            }
        }
        actor.check_session_on(&mut tx).await?;
        tx.commit().await?;
        tracing::debug!(job_id = %fetch.id, status = ?fetch.status, "Artwork request read");
        Ok(fetch)
    }
}

async fn read_on(
    connection: &mut SqliteConnection,
    account: &str,
    id: &str,
) -> Result<ArtworkFetch> {
    let (id, recording_id, status, attempts, error, created_at, updated_at): (String, String, String, i64, Option<String>, i64, i64) =
        sqlx::query_as("SELECT id,recording_id,status,attempts,error,created_at,updated_at FROM artwork_fetches WHERE id = ? AND user_id = ?")
            .bind(id).bind(account).fetch_optional(connection).await?.ok_or(Error::NotFound)?;
    let status = match status.as_str() {
        "queued" => ArtworkFetchStatus::Queued,
        "running" => ArtworkFetchStatus::Running,
        "ready" => ArtworkFetchStatus::Ready,
        "failed" => ArtworkFetchStatus::Failed,
        "cancelled" => ArtworkFetchStatus::Cancelled,
        _ => return Err(Error::Internal("Stored artwork request is invalid".into())),
    };
    Ok(ArtworkFetch {
        id,
        recording_id,
        status,
        attempts: attempts as u32,
        error,
        created_at,
        updated_at,
    })
}

async fn capacity(connection: &mut SqliteConnection, account: &str) -> Result<()> {
    let (account_assets, account_bytes): (i64, i64) = sqlx::query_as(
        "SELECT count(*),coalesce(sum(length(bytes)),0) FROM artwork_assets WHERE user_id = ?",
    )
    .bind(account)
    .fetch_one(&mut *connection)
    .await?;
    let (global_assets, global_bytes): (i64, i64) =
        sqlx::query_as("SELECT count(*),coalesce(sum(length(bytes)),0) FROM artwork_assets")
            .fetch_one(&mut *connection)
            .await?;
    let (account_pending, account_reserved): (i64, i64) = sqlx::query_as(
        "SELECT count(*),coalesce(sum(reservation_bytes),0) FROM artwork_fetches WHERE user_id = ? AND status IN ('queued','running')",
    ).bind(account).fetch_one(&mut *connection).await?;
    let (global_pending, global_reserved): (i64, i64) = sqlx::query_as(
        "SELECT count(*),coalesce(sum(reservation_bytes),0) FROM artwork_fetches WHERE status IN ('queued','running')",
    ).fetch_one(connection).await?;
    if account_pending >= 16 || global_pending >= 128 {
        return Err(Error::Conflict(
            "Artwork requests are full. Try again after pending requests finish.".into(),
        ));
    }
    if account_bytes + account_reserved + RESERVATION_BYTES > 32 * 1024 * 1024
        || global_bytes + global_reserved + RESERVATION_BYTES > 256 * 1024 * 1024
        || account_assets + account_pending >= 1024
        || global_assets + global_pending >= 8192
    {
        return Err(Error::Conflict(
            "Artwork storage is full. Existing artwork has been retained.".into(),
        ));
    }
    Ok(())
}

pub(super) async fn maintain(connection: &mut SqliteConnection) -> Result<()> {
    sqlx::query(
        "UPDATE artwork_fetches SET status = 'cancelled',reservation_bytes = 0,error = 'authority_changed',updated_at = unixepoch()
         WHERE status IN ('queued','running') AND id NOT IN (SELECT id FROM current_artwork_fetch_authority)",
    ).execute(&mut *connection).await?;
    sqlx::query(
        "UPDATE artwork_fetches SET status = CASE WHEN attempts < 3 THEN 'queued' ELSE 'failed' END,
            reservation_bytes = CASE WHEN attempts < 3 THEN 2097152 ELSE 0 END,
            error = CASE WHEN attempts < 3 THEN NULL ELSE 'attempts_exhausted' END,
            lease_token = NULL,lease_until = NULL,next_attempt_at = unixepoch() + 2,updated_at = unixepoch()
         WHERE status = 'running' AND lease_until <= unixepoch()",
    ).execute(&mut *connection).await?;
    sqlx::query("UPDATE artwork_fetches SET lease_token = NULL,lease_until = NULL WHERE status IN ('ready','failed','cancelled') AND lease_until <= unixepoch()")
        .execute(&mut *connection).await?;
    sqlx::query("DELETE FROM artwork_fetches WHERE status IN ('ready','failed','cancelled') AND lease_token IS NULL AND updated_at < unixepoch() - 604800 AND NOT EXISTS(SELECT 1 FROM recording_artwork current WHERE current.fetch_id = artwork_fetches.id)")
        .execute(&mut *connection).await?;
    sqlx::query(
        "DELETE FROM artwork_fetches WHERE id IN (
            SELECT id FROM (SELECT id,row_number() OVER (PARTITION BY user_id ORDER BY updated_at DESC,id DESC) AS position
                FROM artwork_fetches WHERE status IN ('ready','failed','cancelled') AND lease_token IS NULL
                AND NOT EXISTS(SELECT 1 FROM recording_artwork current WHERE current.fetch_id = artwork_fetches.id))
            WHERE position > 256)",
    ).execute(&mut *connection).await?;
    sqlx::query(
        "DELETE FROM artwork_fetches WHERE id IN (SELECT id FROM artwork_fetches
            WHERE status IN ('ready','failed','cancelled') AND lease_token IS NULL
            AND NOT EXISTS(SELECT 1 FROM recording_artwork current WHERE current.fetch_id = artwork_fetches.id)
            ORDER BY updated_at DESC,id DESC LIMIT -1 OFFSET 4096)",
    ).execute(&mut *connection).await?;
    sqlx::query("DELETE FROM artwork_assets WHERE NOT EXISTS(SELECT 1 FROM recording_artwork r WHERE r.user_id = artwork_assets.user_id AND r.content_hash = artwork_assets.content_hash)")
        .execute(connection).await?;
    Ok(())
}
