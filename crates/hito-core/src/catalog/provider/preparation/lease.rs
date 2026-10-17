use super::{Catalog, authority};
use crate::{Error, Result, Store};
use sqlx::SqliteConnection;
use std::time::Duration;
use uuid::Uuid;

#[derive(sqlx::FromRow)]
pub(super) struct Lease {
    pub id: String,
    pub user_id: String,
    pub source_id: String,
    pub submission_state: String,
    pub begin_attempts: i64,
    pub poll_failures: i64,
    pub lease_token: String,
}

pub(super) struct Heartbeat {
    pub owned: bool,
    pub authorized: bool,
}

impl Catalog {
    pub(super) async fn claim_source_preparation(&self) -> Result<Option<Lease>> {
        let eligible: bool = sqlx::query_scalar("SELECT
            EXISTS(SELECT 1 FROM source_preparations WHERE next_check_at <= unixepoch()
                AND authority_cancelled = 0 AND phase IN ('queued','preparing','blocked'))
            OR EXISTS(SELECT 1 FROM source_preparations WHERE lease_until <= unixepoch())
            OR EXISTS(SELECT 1 FROM source_preparations preparation WHERE authority_cancelled = 0
                AND phase IN ('queued','preparing','blocked','submission_unknown')
                AND NOT EXISTS(SELECT 1 FROM current_source_preparation_authority authority WHERE authority.id = preparation.id))")
            .fetch_one(&self.store.pool).await?;
        if !eligible {
            return Ok(None);
        }
        let mut tx = self.store.pool.begin_with("BEGIN IMMEDIATE").await?;
        let expired: Vec<String> = sqlx::query_scalar(
            "SELECT id FROM source_preparations
            WHERE lease_until <= unixepoch() ORDER BY lease_until,id LIMIT 16",
        )
        .fetch_all(&mut *tx)
        .await?;
        for id in expired {
            sqlx::query("UPDATE source_preparations SET
                phase = CASE WHEN submission_state = 'submitting' THEN 'submission_unknown' ELSE phase END,
                next_check_at = CASE WHEN submission_state = 'submitting' OR authority_cancelled = 1 THEN NULL ELSE next_check_at END,
                submission_state = CASE WHEN submission_state = 'submitting' THEN 'unknown' ELSE submission_state END,
                lease_token = NULL,lease_until = NULL,updated_at = unixepoch() WHERE id = ?")
                .bind(&id).execute(&mut *tx).await?;
            tracing::debug!(
                event = "source_preparation_lease_recovered",
                preparation_id = id,
                "Expired source preparation lease recovered"
            );
        }
        let stale: Vec<String> = sqlx::query_scalar("SELECT id FROM source_preparations preparation
            WHERE authority_cancelled = 0 AND phase IN ('queued','submitting','preparing','blocked','submission_unknown')
                AND NOT EXISTS(SELECT 1 FROM current_source_preparation_authority authority WHERE authority.id = preparation.id)
            ORDER BY created_at,id LIMIT 16")
            .fetch_all(&mut *tx).await?;
        for id in stale {
            authority::cancel_stale_on(&mut tx, &id).await?;
        }
        sqlx::query(
            "UPDATE source_preparations SET phase = 'blocked',reason = 'provider_unavailable',
            next_check_at = NULL,updated_at = unixepoch() WHERE id IN (
                SELECT id FROM source_preparations WHERE next_check_at <= unixepoch()
                    AND created_at + 86400 <= unixepoch() AND lease_token IS NULL
                    AND phase IN ('queued','preparing','blocked') ORDER BY created_at,id LIMIT 16)",
        )
        .execute(&mut *tx)
        .await?;
        let id: Option<String> = sqlx::query_scalar("SELECT preparation.id FROM source_preparations preparation
            JOIN current_source_preparation_authority authority ON authority.id = preparation.id
            WHERE preparation.phase IN ('queued','preparing','blocked') AND preparation.next_check_at <= unixepoch()
                AND preparation.submission_state IN ('unsubmitted','submitted') AND preparation.lease_token IS NULL
                AND NOT EXISTS(SELECT 1 FROM source_preparations active WHERE active.user_id = preparation.user_id AND active.lease_token IS NOT NULL)
                AND (SELECT count(*) FROM source_preparations active WHERE active.lease_token IS NOT NULL) < 2
            ORDER BY preparation.next_check_at,preparation.created_at,preparation.id LIMIT 1")
            .fetch_optional(&mut *tx).await?;
        let Some(id) = id else {
            tx.commit().await?;
            return Ok(None);
        };
        let token = Uuid::new_v4().to_string();
        sqlx::query(
            "UPDATE source_preparations SET lease_token = ?,lease_until = unixepoch() + 90,
            attempts = attempts + 1,updated_at = unixepoch() WHERE id = ? AND lease_token IS NULL",
        )
        .bind(token)
        .bind(&id)
        .execute(&mut *tx)
        .await?;
        let lease = sqlx::query_as(
            "SELECT id,user_id,source_id,submission_state,begin_attempts,poll_failures,lease_token
            FROM source_preparations WHERE id = ?",
        )
        .bind(id)
        .fetch_one(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(Some(lease))
    }
}

impl Lease {
    pub(super) async fn check_on(&self, connection: &mut SqliteConnection) -> Result<()> {
        let owned: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM source_preparations preparation
            JOIN current_source_preparation_authority authority ON authority.id = preparation.id
            WHERE preparation.id = ? AND preparation.lease_token = ? AND preparation.lease_until > unixepoch())")
            .bind(&self.id).bind(&self.lease_token).fetch_one(connection).await?;
        if owned {
            Ok(())
        } else {
            Err(Error::Conflict(
                "This source preparation lease is no longer current".into(),
            ))
        }
    }

    pub(super) async fn mark_submitting(&self, store: &Store) -> Result<()> {
        let mut tx = store.pool.begin_with("BEGIN IMMEDIATE").await?;
        self.check_on(&mut tx).await?;
        let changed = sqlx::query("UPDATE source_preparations SET phase = 'submitting',submission_state = 'submitting',
            dispatch_id = ?,begin_attempts = begin_attempts + 1,updated_at = unixepoch()
            WHERE id = ? AND lease_token = ? AND submission_state = 'unsubmitted' AND begin_attempts < 3")
            .bind(&self.lease_token).bind(&self.id).bind(&self.lease_token).execute(&mut *tx).await?.rows_affected();
        if changed != 1 {
            return Err(Error::Conflict(
                "This source preparation cannot be submitted".into(),
            ));
        }
        tx.commit().await?;
        Ok(())
    }

    pub(super) async fn heartbeat(&self, store: &Store) -> Result<Heartbeat> {
        tokio::time::timeout(Duration::from_secs(1), async {
            let mut tx = store.pool.begin_with("BEGIN IMMEDIATE").await?;
            let renewed = sqlx::query(
                "UPDATE source_preparations SET lease_until = unixepoch() + 90
                WHERE id = ? AND lease_token = ? AND lease_until > unixepoch()",
            )
            .bind(&self.id)
            .bind(&self.lease_token)
            .execute(&mut *tx)
            .await?
            .rows_affected();
            authority::cancel_stale_on(&mut tx, &self.id).await?;
            let authorized = match authority::check_on(&mut tx, &self.id).await {
                Ok(()) => true,
                Err(Error::Conflict(_)) => false,
                Err(error) => return Err(error),
            };
            tx.commit().await?;
            Ok(Heartbeat {
                owned: renewed == 1,
                authorized,
            })
        })
        .await
        .unwrap_or(Err(Error::Upstream(
            "Source preparation authority could not be checked in time",
        )))
    }
}
