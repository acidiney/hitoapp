use super::{Catalog, jobs};
use crate::{Actor, Error, Result, Store};
use sqlx::SqliteConnection;
use uuid::Uuid;

#[derive(sqlx::FromRow)]
pub(super) struct Lease {
    pub(super) id: String,
    pub(super) user_id: String,
    session_hash: String,
    pub(super) recording_id: String,
    pub(super) observation_id: String,
    pub(super) installation_id: String,
    pub(super) installation_revision: i64,
    pub(super) service_id: String,
    pub(super) service_revision: i64,
    pub(super) provider_recording_id: String,
    pub(super) attempts: i64,
    pub(super) lease_token: String,
}

impl Catalog {
    pub(super) async fn claim_artwork_job(&self) -> Result<Option<Lease>> {
        let mut tx = self.store.pool.begin_with("BEGIN IMMEDIATE").await?;
        jobs::maintain(&mut tx).await?;
        let id: Option<String> = sqlx::query_scalar(
            "SELECT candidate.id FROM artwork_fetches candidate
             WHERE candidate.status = 'queued' AND candidate.attempts < 3 AND candidate.next_attempt_at <= unixepoch()
             AND NOT EXISTS(SELECT 1 FROM artwork_fetches active WHERE active.user_id = candidate.user_id
                AND active.lease_token IS NOT NULL AND active.lease_until > unixepoch())
             AND (SELECT count(*) FROM artwork_fetches active WHERE active.lease_token IS NOT NULL
                AND active.lease_until > unixepoch()) < 4
             ORDER BY candidate.created_at,candidate.id LIMIT 1",
        ).fetch_optional(&mut *tx).await?;
        let Some(id) = id else {
            tx.commit().await?;
            return Ok(None);
        };
        let token = Uuid::new_v4().to_string();
        sqlx::query("UPDATE artwork_fetches SET status = 'running',attempts = attempts + 1,lease_token = ?,lease_until = unixepoch() + 60,error = NULL,updated_at = unixepoch() WHERE id = ? AND status = 'queued'")
            .bind(&token).bind(&id).execute(&mut *tx).await?;
        let lease: Lease = sqlx::query_as(
            "SELECT j.id,j.user_id,j.session_hash,j.recording_id,j.observation_id,j.installation_id,j.installation_revision,
                j.service_id,j.service_revision,o.provider_recording_id,j.attempts,j.lease_token
             FROM artwork_fetches j JOIN metadata_observations o ON o.id = j.observation_id WHERE j.id = ?",
        ).bind(&id).fetch_one(&mut *tx).await?;
        tx.commit().await?;
        tracing::debug!(job_id = %id, account_id = %lease.user_id, attempt = lease.attempts, "Artwork lease acquired");
        Ok(Some(lease))
    }
}

impl Lease {
    pub(super) async fn actor(&self, store: &Store) -> Result<Actor> {
        tokio::time::timeout(std::time::Duration::from_secs(1), async {
            self.check_on(&mut *store.pool.acquire().await?).await
        })
        .await
        .unwrap_or(Err(Error::Upstream(
            "Artwork authorization could not be checked in time",
        )))
    }

    pub(super) async fn check_on(&self, connection: &mut SqliteConnection) -> Result<Actor> {
        let current: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM artwork_fetches j JOIN current_artwork_fetch_authority authority ON authority.id = j.id
             WHERE j.id = ? AND j.lease_token = ? AND j.status = 'running' AND j.lease_until > unixepoch())",
        ).bind(&self.id).bind(&self.lease_token).fetch_one(&mut *connection).await?;
        if !current {
            return Err(Error::Conflict(
                "This artwork request is no longer current".into(),
            ));
        }
        Actor::resume_session_on(connection, &self.user_id, &self.session_hash).await
    }
}
