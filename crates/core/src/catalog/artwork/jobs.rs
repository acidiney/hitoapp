use super::{Catalog, playlist, release};
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

impl TryFrom<String> for ArtworkFetchStatus {
    type Error = Error;

    fn try_from(status: String) -> Result<Self> {
        match status.as_str() {
            "queued" => Ok(Self::Queued),
            "running" => Ok(Self::Running),
            "ready" => Ok(Self::Ready),
            "failed" => Ok(Self::Failed),
            "cancelled" => Ok(Self::Cancelled),
            _ => Err(Error::Internal("Stored artwork request is invalid".into())),
        }
    }
}

#[derive(Clone, Copy)]
pub(super) enum EntityKind {
    Recording,
    Release,
    Playlist,
}

impl EntityKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Recording => "recording",
            Self::Release => "release",
            Self::Playlist => "playlist",
        }
    }
}

#[derive(sqlx::FromRow)]
pub(super) struct FetchRow {
    pub id: String,
    pub entity_id: String,
    #[sqlx(try_from = "String")]
    pub status: ArtworkFetchStatus,
    #[sqlx(try_from = "i64")]
    pub attempts: u32,
    pub error: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
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

impl From<FetchRow> for ArtworkFetch {
    fn from(row: FetchRow) -> Self {
        Self {
            id: row.id,
            recording_id: row.entity_id,
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
            "SELECT id,session_hash FROM artwork_fetches WHERE user_id = ? AND recording_id = ? AND entity_kind = 'recording'
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
            let fetch: ArtworkFetch = fetch_row(&mut tx, actor.id(), &id, EntityKind::Recording)
                .await?
                .into();
            tx.commit().await?;
            actor.check_session(&self.store).await?;
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
            let fetch: ArtworkFetch = fetch_row(&mut tx, actor.id(), &id, EntityKind::Recording)
                .await?
                .into();
            tx.commit().await?;
            actor.check_session(&self.store).await?;
            tracing::info!(job_id = %id, outcome = "already_retained", elapsed_ms = started.elapsed().as_millis() as u64, "Artwork request accepted");
            return Ok(fetch);
        }
        capacity(&mut tx, actor.id()).await?;
        let provider = super::availability::approved_provider(&mut tx, actor.id(), recording)
            .await?
            .ok_or_else(|| {
                Error::Invalid("This recording has no approved artwork provider".into())
            })?;
        let id = Uuid::new_v4().to_string();
        sqlx::query(
            "INSERT INTO artwork_fetches(id,user_id,recording_id,session_hash,observation_id,binding_id,binding_revision,
                installation_id,installation_revision,service_id,service_revision,status,reservation_bytes)
             VALUES (?,?,?,?,?,?,?,?,?,?,?,'queued',?)",
        ).bind(&id).bind(actor.id()).bind(recording).bind(actor.session_binding())
            .bind(provider.observation_id).bind(provider.binding_id).bind(provider.binding_revision)
            .bind(provider.installation_id).bind(provider.installation_revision)
            .bind(provider.service_id).bind(provider.service_revision).bind(RESERVATION_BYTES)
            .execute(&mut *tx).await?;
        actor.check_session_on(&mut tx).await?;
        let fetch: ArtworkFetch = fetch_row(&mut tx, actor.id(), &id, EntityKind::Recording)
            .await?
            .into();
        tx.commit().await?;
        actor.check_session(&self.store).await?;
        tracing::info!(job_id = %id, outcome = "queued", elapsed_ms = started.elapsed().as_millis() as u64, "Artwork request accepted");
        Ok(fetch)
    }

    #[tracing::instrument(level = "debug", skip_all, err(Debug), fields(account_id = actor.id()))]
    pub async fn artwork_fetch(&self, actor: &Actor, id: &str) -> Result<ArtworkFetch> {
        let fetch: ArtworkFetch = self
            .read_artwork_fetch(actor, id, EntityKind::Recording)
            .await?
            .into();
        tracing::debug!(job_id = %fetch.id, status = ?fetch.status, "Artwork request read");
        Ok(fetch)
    }

    pub(super) async fn read_artwork_fetch(
        &self,
        actor: &Actor,
        id: &str,
        kind: EntityKind,
    ) -> Result<FetchRow> {
        let mut tx = self.store.pool.begin().await?;
        actor.check_session_on(&mut tx).await?;
        let mut row = fetch_row(&mut tx, actor.id(), id, kind).await?;
        match kind {
            EntityKind::Recording => recording_access(&mut tx, actor, &row.entity_id).await?,
            EntityKind::Release => release::access_on(&mut tx, actor, &row.entity_id).await?,
            EntityKind::Playlist => playlist::access_on(&mut tx, actor, &row.entity_id).await?,
        }
        observe_status(&mut tx, actor.id(), &mut row).await?;
        tx.commit().await?;
        actor.check_session(&self.store).await?;
        Ok(row)
    }
}

async fn observe_status(
    connection: &mut SqliteConnection,
    account: &str,
    row: &mut FetchRow,
) -> Result<()> {
    if !matches!(
        &row.status,
        ArtworkFetchStatus::Queued | ArtworkFetchStatus::Running
    ) {
        return Ok(());
    }
    let (current, lease_expired): (bool, bool) = sqlx::query_as(
        "SELECT EXISTS(SELECT 1 FROM current_artwork_fetch_authority authority WHERE authority.id = j.id),
            coalesce(j.lease_until <= unixepoch(),0)
         FROM artwork_fetches j WHERE j.id = ? AND j.user_id = ?",
    ).bind(&row.id).bind(account).fetch_one(connection).await?;
    if !current {
        row.status = ArtworkFetchStatus::Cancelled;
        row.error = Some("authority_changed".into());
    } else if lease_expired {
        if row.attempts < 3 {
            row.status = ArtworkFetchStatus::Queued;
            row.error = None;
        } else {
            row.status = ArtworkFetchStatus::Failed;
            row.error = Some("attempts_exhausted".into());
        }
    }
    Ok(())
}

pub(super) async fn fetch_row(
    connection: &mut SqliteConnection,
    account: &str,
    id: &str,
    kind: EntityKind,
) -> Result<FetchRow> {
    sqlx::query_as(
        "SELECT id,
            CASE entity_kind WHEN 'recording' THEN recording_id WHEN 'release' THEN release_id ELSE playlist_id END AS entity_id,
            status,attempts,error,created_at,updated_at
         FROM artwork_fetches WHERE id = ? AND user_id = ? AND entity_kind = ?",
    )
    .bind(id)
    .bind(account)
    .bind(kind.as_str())
    .fetch_optional(connection)
    .await?
    .ok_or(Error::NotFound)
}

pub(super) async fn capacity(connection: &mut SqliteConnection, account: &str) -> Result<()> {
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
        return Err(Error::ArtworkAdmissionBusy);
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
         WHERE status IN ('queued','running') AND NOT EXISTS(
            SELECT 1 FROM current_artwork_fetch_authority authority WHERE authority.id = artwork_fetches.id)",
    ).execute(&mut *connection).await?;
    sqlx::query(
        "UPDATE artwork_fetches SET status = CASE WHEN attempts < 3 THEN 'queued' ELSE 'failed' END,
            reservation_bytes = CASE WHEN attempts < 3 THEN 2097152 ELSE 0 END,
            error = CASE WHEN attempts < 3 THEN NULL ELSE 'attempts_exhausted' END,
            lease_token = NULL,lease_until = NULL,next_attempt_at = unixepoch() + 2,updated_at = unixepoch()
         WHERE status = 'running' AND lease_token IS NOT NULL AND lease_until <= unixepoch()",
    ).execute(&mut *connection).await?;
    sqlx::query("UPDATE artwork_fetches INDEXED BY artwork_expired_lease SET lease_token = NULL,lease_until = NULL WHERE lease_token IS NOT NULL AND lease_until <= unixepoch() AND status IN ('ready','failed','cancelled')")
        .execute(&mut *connection).await?;
    trim_history(
        connection,
        "SELECT id FROM artwork_fetches INDEXED BY artwork_history
         WHERE retained = 0 AND status IN ('ready','failed','cancelled') AND lease_token IS NULL
            AND updated_at < unixepoch() - 604800",
    )
    .await?;
    trim_history(connection,
        "SELECT id FROM (SELECT id,row_number() OVER (PARTITION BY user_id ORDER BY updated_at DESC,id DESC) AS position
                FROM artwork_fetches INDEXED BY artwork_history_account
                WHERE retained = 0 AND status IN ('ready','failed','cancelled') AND lease_token IS NULL
                AND NOT EXISTS(SELECT 1 FROM retained_artwork_references current WHERE current.fetch_id = artwork_fetches.id))
            WHERE position > 256",
    ).await?;
    trim_history(connection,
        "SELECT id FROM artwork_fetches INDEXED BY artwork_history
            WHERE retained = 0 AND status IN ('ready','failed','cancelled') AND lease_token IS NULL
            AND NOT EXISTS(SELECT 1 FROM retained_artwork_references current WHERE current.fetch_id = artwork_fetches.id)
            ORDER BY updated_at DESC,id DESC LIMIT -1 OFFSET 4096",
    ).await?;
    super::assets::prune(connection, None).await?;
    Ok(())
}

async fn trim_history(connection: &mut SqliteConnection, selector: &str) -> Result<()> {
    let obsolete: Vec<String> = sqlx::query_scalar(selector)
        .fetch_all(&mut *connection)
        .await?;
    for id in obsolete {
        sqlx::query("DELETE FROM artwork_fetches WHERE id = ? AND retained = 0
            AND status IN ('ready','failed','cancelled') AND lease_token IS NULL
            AND NOT EXISTS(SELECT 1 FROM retained_artwork_references current WHERE current.fetch_id = artwork_fetches.id)")
            .bind(id).execute(&mut *connection).await?;
    }
    Ok(())
}
