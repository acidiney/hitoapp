use super::super::{Podcasts, subscriptions::queue_refresh};
use crate::{Error, Result, Store, remote_http::ConditionalValidators};
use sqlx::SqliteConnection;
use std::{collections::BTreeSet, time::Duration};
use uuid::Uuid;

#[derive(sqlx::FromRow)]
pub(super) struct Lease {
    pub(super) id: String,
    pub(super) subscription_id: String,
    pub(super) user_id: String,
    pub(super) library_id: String,
    pub(super) generation: i64,
    pub(super) lease_token: String,
    pub(super) attempts: i64,
    pub(super) fetch_url: String,
    pub(super) content_revision: i64,
    validator_url: Option<String>,
    etag: Option<String>,
    last_modified: Option<String>,
}

impl Podcasts {
    pub(super) async fn claim(&self) -> Result<Option<Lease>> {
        let eligible: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM podcast_refresh_jobs WHERE status='queued' AND next_attempt_at<=unixepoch())
            OR EXISTS(SELECT 1 FROM podcast_refresh_jobs WHERE status='running' AND lease_until<=unixepoch())
            OR EXISTS(SELECT 1 FROM podcast_subscriptions p JOIN current_podcast_authority a ON a.id=p.id
                WHERE p.active=1 AND p.next_refresh_at<=unixepoch()
                AND NOT EXISTS(SELECT 1 FROM podcast_refresh_jobs j WHERE j.subscription_id=p.id AND j.status IN ('queued','running')))")
            .fetch_one(&self.store.pool).await?;
        if !eligible {
            return Ok(None);
        }
        let mut tx = self.store.pool.begin_with("BEGIN IMMEDIATE").await?;
        let recovered: Vec<(String, i64, String)> = sqlx::query_as("UPDATE podcast_refresh_jobs
            SET status=CASE WHEN attempts>=3 THEN 'failed' ELSE 'queued' END,lease_token=NULL,lease_until=NULL,error='lease_expired',updated_at=unixepoch()
            WHERE id IN (SELECT id FROM podcast_refresh_jobs WHERE status='running' AND lease_until<=unixepoch() ORDER BY lease_until,id LIMIT 32)
            RETURNING subscription_id,generation,status")
            .fetch_all(&mut *tx).await?;
        let mut terminal = BTreeSet::new();
        for (subscription, generation, status) in &recovered {
            if status == "failed" {
                sqlx::query("UPDATE podcast_subscriptions SET error='lease_expired',next_refresh_at=unixepoch()+3600 WHERE id=? AND generation=?")
                    .bind(subscription).bind(generation).execute(&mut *tx).await?;
                terminal.insert(subscription.clone());
            }
        }
        let cancelled: Vec<(String, i64)> = sqlx::query_as("UPDATE podcast_refresh_jobs SET status='cancelled',error='authority_changed',updated_at=unixepoch()
            WHERE status='queued' AND NOT EXISTS(SELECT 1 FROM podcast_subscriptions p JOIN current_podcast_authority a ON a.id=p.id
                WHERE p.id=podcast_refresh_jobs.subscription_id AND p.generation=podcast_refresh_jobs.generation AND p.user_id=podcast_refresh_jobs.user_id)
            RETURNING subscription_id,generation")
            .fetch_all(&mut *tx).await?;
        for (subscription, generation) in &cancelled {
            sqlx::query("UPDATE podcast_subscriptions SET error='authority_changed',next_refresh_at=unixepoch()+3600 WHERE id=? AND generation=?")
                .bind(subscription).bind(generation).execute(&mut *tx).await?;
            terminal.insert(subscription.clone());
        }
        for subscription in terminal {
            prune(&mut tx, &subscription).await?;
        }
        if !recovered.is_empty() || !cancelled.is_empty() {
            tracing::debug!(
                recovered_jobs = recovered.len(),
                cancelled_jobs = cancelled.len(),
                "Podcast refresh lease recovery finished"
            );
        }
        let due: Option<(String, String, i64)> = sqlx::query_as("SELECT p.id,p.user_id,p.generation FROM podcast_subscriptions p
            JOIN current_podcast_authority a ON a.id=p.id
            WHERE p.active=1 AND p.next_refresh_at<=unixepoch()
                AND NOT EXISTS(SELECT 1 FROM podcast_refresh_jobs j WHERE j.subscription_id=p.id AND j.status IN ('queued','running'))
                AND (SELECT count(*) FROM podcast_refresh_jobs j WHERE j.user_id=p.user_id AND j.status IN ('queued','running'))<16
                AND (SELECT count(*) FROM podcast_refresh_jobs j WHERE j.status IN ('queued','running'))<128
            ORDER BY p.next_refresh_at,p.id LIMIT 1")
            .fetch_optional(&mut *tx).await?;
        if let Some((subscription, account, generation)) = due {
            let job = queue_refresh(&mut tx, &account, &subscription, generation).await?;
            tracing::debug!(job_id = %job, subscription_id = %subscription, account_id = %account, "Scheduled podcast refresh admitted");
        }
        let id: Option<String> = sqlx::query_scalar("SELECT j.id FROM podcast_refresh_jobs j WHERE j.status='queued' AND j.attempts<3 AND j.next_attempt_at<=unixepoch()
            AND NOT EXISTS(SELECT 1 FROM podcast_refresh_jobs active WHERE active.user_id=j.user_id AND active.status='running')
            AND (SELECT count(*) FROM podcast_refresh_jobs active WHERE active.status='running')<2 ORDER BY j.created_at,j.id LIMIT 1")
            .fetch_optional(&mut *tx).await?;
        let Some(id) = id else {
            tx.commit().await?;
            return Ok(None);
        };
        sqlx::query("UPDATE podcast_refresh_jobs SET status='running',attempts=attempts+1,lease_token=?,lease_until=unixepoch()+90,error=NULL,updated_at=unixepoch() WHERE id=? AND status='queued'")
            .bind(Uuid::new_v4().to_string()).bind(&id).execute(&mut *tx).await?;
        let lease = sqlx::query_as("SELECT j.id,j.subscription_id,j.user_id,j.generation,j.lease_token,j.attempts,
            p.library_id,p.fetch_url,p.content_revision,p.validator_url,p.etag,p.last_modified
            FROM podcast_refresh_jobs j JOIN podcast_subscriptions p ON p.id=j.subscription_id AND p.user_id=j.user_id WHERE j.id=?")
            .bind(id).fetch_one(&mut *tx).await?;
        tx.commit().await?;
        Ok(Some(lease))
    }
}

impl Lease {
    pub(super) fn validators(&self) -> ConditionalValidators {
        ConditionalValidators::stored(
            self.validator_url.as_deref(),
            self.etag.as_deref(),
            self.last_modified.as_deref(),
        )
    }

    pub(super) async fn check(&self, store: &Store) -> Result<()> {
        tokio::time::timeout(Duration::from_secs(1), async {
            self.check_on(&mut *store.pool.acquire().await?).await
        })
        .await
        .unwrap_or(Err(Error::Upstream(
            "Podcast refresh authority could not be checked in time",
        )))
    }

    pub(super) async fn check_on(&self, connection: &mut SqliteConnection) -> Result<()> {
        let current: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM podcast_refresh_jobs j
            JOIN podcast_subscriptions p ON p.id=j.subscription_id AND p.user_id=j.user_id AND p.generation=j.generation
            JOIN current_podcast_authority a ON a.id=p.id
            WHERE j.id=? AND j.subscription_id=? AND j.user_id=? AND j.generation=? AND j.status='running' AND j.lease_token=? AND j.lease_until>unixepoch())")
            .bind(&self.id).bind(&self.subscription_id).bind(&self.user_id).bind(self.generation).bind(&self.lease_token)
            .fetch_one(connection).await?;
        if current {
            Ok(())
        } else {
            Err(Error::Conflict(
                "This podcast refresh is no longer current".into(),
            ))
        }
    }
}

pub(super) async fn prune(connection: &mut SqliteConnection, subscription: &str) -> Result<()> {
    let removed = sqlx::query("DELETE FROM podcast_refresh_jobs WHERE id IN (
        SELECT j.id FROM podcast_refresh_jobs j WHERE j.subscription_id=? AND j.status IN ('complete','failed','cancelled')
            AND NOT EXISTS(SELECT 1 FROM podcast_subscription_receipts r WHERE r.job_id=j.id)
            AND NOT EXISTS(SELECT 1 FROM podcast_refresh_receipts r WHERE r.job_id=j.id)
        ORDER BY j.created_at DESC,j.rowid DESC LIMIT -1 OFFSET 32)")
        .bind(subscription).execute(connection).await?.rows_affected();
    if removed > 0 {
        tracing::debug!(
            pruned_jobs = removed,
            "Unreferenced podcast refresh history trimmed"
        );
    }
    Ok(())
}
