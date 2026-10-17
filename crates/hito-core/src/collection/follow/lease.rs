use super::{Collection, jobs};
use crate::{Error, Result, Store};
use sqlx::SqliteConnection;
use uuid::Uuid;

#[derive(sqlx::FromRow)]
pub(super) struct Lease {
    pub(super) id: String,
    pub(super) user_id: String,
    pub(super) playlist_id: String,
    pub(super) generation: i64,
    pub(super) installation_id: String,
    pub(super) installation_revision: i64,
    pub(super) service_id: String,
    pub(super) service_revision: i64,
    pub(super) provider: String,
    pub(super) provider_playlist_id: String,
    pub(super) attempts: i64,
    pub(super) lease_token: String,
}

impl Collection {
    pub(super) async fn claim_follow_job(&self) -> Result<Option<Lease>> {
        let eligible: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM playlist_follow_jobs WHERE status = 'queued' AND next_attempt_at <= unixepoch())
            OR EXISTS(SELECT 1 FROM playlist_follow_jobs WHERE status = 'running' AND lease_until <= unixepoch())
            OR EXISTS(SELECT 1 FROM playlist_follows f JOIN current_playlist_follow_authority authority ON authority.playlist_id = f.playlist_id
                WHERE f.active = 1 AND f.next_refresh_at <= unixepoch()
                AND NOT EXISTS(SELECT 1 FROM playlist_follow_jobs j WHERE j.playlist_id = f.playlist_id AND j.status IN ('queued','running')))")
            .fetch_one(&self.store.pool).await?;
        if !eligible {
            return Ok(None);
        }
        let mut tx = self.store.pool.begin_with("BEGIN IMMEDIATE").await?;
        sqlx::query("UPDATE playlist_follow_jobs SET status = CASE WHEN attempts >= 3 THEN 'failed' ELSE 'queued' END,
            lease_token = NULL,lease_until = NULL,error = 'lease_expired',updated_at = unixepoch()
            WHERE status = 'running' AND lease_until <= unixepoch()")
            .execute(&mut *tx).await?;
        sqlx::query("UPDATE playlist_follows SET error = 'refresh_failed',next_refresh_at = unixepoch() + interval_seconds
            WHERE next_refresh_at <= unixepoch() AND EXISTS(SELECT 1 FROM playlist_follow_jobs j WHERE j.playlist_id = playlist_follows.playlist_id
                AND j.status = 'failed' AND j.error = 'lease_expired' AND j.updated_at = unixepoch())")
            .execute(&mut *tx).await?;
        sqlx::query("UPDATE playlist_follow_jobs SET status = 'cancelled',error = 'authority_changed',updated_at = unixepoch()
            WHERE status = 'queued' AND NOT EXISTS(SELECT 1 FROM playlist_follows f JOIN current_playlist_follow_authority authority ON authority.playlist_id = f.playlist_id
                WHERE f.playlist_id = playlist_follow_jobs.playlist_id AND f.generation = playlist_follow_jobs.generation)")
            .execute(&mut *tx).await?;
        let due: Option<(String, String, i64)> = sqlx::query_as("SELECT f.playlist_id,f.user_id,f.generation FROM playlist_follows f
            JOIN current_playlist_follow_authority authority ON authority.playlist_id = f.playlist_id
            WHERE f.active = 1 AND f.next_refresh_at <= unixepoch()
                AND NOT EXISTS(SELECT 1 FROM playlist_follow_jobs j WHERE j.playlist_id = f.playlist_id AND j.status IN ('queued','running'))
                AND (SELECT count(*) FROM playlist_follow_jobs j WHERE j.user_id = f.user_id AND j.status IN ('queued','running')) < ?
                AND (SELECT count(*) FROM playlist_follow_jobs j WHERE j.status IN ('queued','running')) < ?
            ORDER BY f.next_refresh_at,f.playlist_id LIMIT 1")
            .bind(jobs::MAX_ACCOUNT_PENDING).bind(jobs::MAX_PENDING).fetch_optional(&mut *tx).await?;
        if let Some((playlist, account, generation)) = due {
            sqlx::query("INSERT INTO playlist_follow_jobs(id,user_id,playlist_id,generation,status) VALUES (?,?,?,?,'queued')")
                .bind(Uuid::new_v4().to_string()).bind(account).bind(playlist).bind(generation).execute(&mut *tx).await?;
        }
        let id: Option<String> = sqlx::query_scalar("SELECT candidate.id FROM playlist_follow_jobs candidate
            WHERE candidate.status = 'queued' AND candidate.attempts < 3 AND candidate.next_attempt_at <= unixepoch()
                AND NOT EXISTS(SELECT 1 FROM playlist_follow_jobs active WHERE active.user_id = candidate.user_id AND active.status = 'running')
                AND (SELECT count(*) FROM playlist_follow_jobs active WHERE active.status = 'running') < 4
            ORDER BY candidate.created_at,candidate.id LIMIT 1")
            .fetch_optional(&mut *tx).await?;
        let Some(id) = id else {
            tx.commit().await?;
            return Ok(None);
        };
        let token = Uuid::new_v4().to_string();
        sqlx::query("UPDATE playlist_follow_jobs SET status = 'running',attempts = attempts + 1,lease_token = ?,lease_until = unixepoch() + 90,error = NULL,updated_at = unixepoch() WHERE id = ? AND status = 'queued'")
            .bind(&token).bind(&id).execute(&mut *tx).await?;
        let lease: Lease = sqlx::query_as("SELECT j.id,j.user_id,j.playlist_id,j.generation,j.attempts,j.lease_token,
            f.installation_id,f.installation_revision,f.service_id,f.service_revision,f.provider,f.provider_playlist_id
            FROM playlist_follow_jobs j JOIN playlist_follows f ON f.playlist_id = j.playlist_id WHERE j.id = ?")
            .bind(&id).fetch_one(&mut *tx).await?;
        tx.commit().await?;
        tracing::debug!(job_id = %lease.id, account_id = %lease.user_id, attempt = lease.attempts, "Follow refresh lease acquired");
        Ok(Some(lease))
    }
}

impl Lease {
    pub(super) async fn check(&self, store: &Store) -> Result<()> {
        tokio::time::timeout(std::time::Duration::from_secs(1), async {
            self.check_on(&mut *store.pool.acquire().await?).await
        })
        .await
        .unwrap_or(Err(Error::Upstream(
            "Playlist refresh authority could not be checked in time",
        )))
    }

    pub(super) async fn check_on(&self, connection: &mut SqliteConnection) -> Result<()> {
        let valid: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM playlist_follow_jobs j
            JOIN playlist_follows f ON f.playlist_id = j.playlist_id AND f.generation = j.generation
            JOIN current_playlist_follow_authority authority ON authority.playlist_id = f.playlist_id
            WHERE j.id = ? AND j.user_id = ? AND j.playlist_id = ? AND j.generation = ?
                AND j.status = 'running' AND j.lease_token = ? AND j.lease_until > unixepoch()
                AND f.installation_id = ? AND f.installation_revision = ? AND f.service_id = ? AND f.service_revision = ? AND f.provider = ?)")
            .bind(&self.id).bind(&self.user_id).bind(&self.playlist_id).bind(self.generation).bind(&self.lease_token)
            .bind(&self.installation_id).bind(self.installation_revision).bind(&self.service_id).bind(self.service_revision).bind(&self.provider)
            .fetch_one(connection).await?;
        if !valid {
            return Err(Error::Conflict(
                "This playlist refresh is no longer current".into(),
            ));
        }
        Ok(())
    }
}
