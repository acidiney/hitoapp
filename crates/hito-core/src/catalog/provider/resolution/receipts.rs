use super::{
    SourceResolutionOutcome,
    authority::{self, Target},
};
use crate::{Actor, Catalog, Error, Result, Store};
use sqlx::{SqliteConnection, SqlitePool};
use uuid::Uuid;

pub(super) enum Admission {
    Execute(Box<Attempt>),
    Observe { id: String, outcome: Option<String> },
    Busy,
}

pub(super) struct Attempt {
    pub id: String,
    pub token: String,
    pub target: Target,
    pub target_digest: String,
}

pub(super) struct Lease {
    pool: SqlitePool,
    token: String,
}

impl Lease {
    pub(super) fn new(store: &Store) -> Self {
        Self {
            pool: store.pool.clone(),
            token: Uuid::new_v4().to_string(),
        }
    }

    pub(super) fn token(&self) -> &str {
        &self.token
    }

    pub(super) async fn cancel(&self) -> Result<()> {
        abandon(&self.pool, &self.token).await
    }
}

async fn abandon(pool: &SqlitePool, token: &str) -> Result<()> {
    sqlx::query("UPDATE source_resolutions SET outcome = 'cancelled',completed_at = unixepoch(),lease_token = NULL,lease_until = NULL WHERE outcome IS NULL AND lease_token = ?")
        .bind(token).execute(pool).await?;
    Ok(())
}

impl Catalog {
    pub(super) async fn admit_resolution(
        &self,
        actor: &Actor,
        recording: &str,
        request: Uuid,
        token: &str,
    ) -> Result<Admission> {
        let mut tx = self.store.pool.begin_with("BEGIN IMMEDIATE").await?;
        authority::access(&mut tx, actor, recording).await?;
        let receipt: Option<(String, String, Option<String>)> = sqlx::query_as("SELECT receipt.recording_id,resolution.id,resolution.outcome FROM source_resolution_receipts receipt
            JOIN source_resolutions resolution ON resolution.id = receipt.resolution_id AND resolution.user_id = receipt.user_id AND resolution.recording_id = receipt.recording_id
            WHERE receipt.user_id = ? AND receipt.request_id = ?")
            .bind(actor.id()).bind(request.to_string()).fetch_optional(&mut *tx).await?;
        if let Some((saved, id, outcome)) = receipt {
            if saved != recording {
                return Err(Error::Conflict(
                    "A source resolution request ID was reused for another recording".into(),
                ));
            }
            if outcome.is_some() {
                tx.commit().await?;
                return Ok(Admission::Observe { id, outcome });
            }
            let target = authority::capture(&mut tx, actor, recording).await?;
            let admitted = reclaim(&mut tx, &id, token, target).await?;
            tx.commit().await?;
            return Ok(admitted);
        }
        let target = authority::capture(&mut tx, actor, recording).await?;
        let pending: Option<String> = sqlx::query_scalar("SELECT id FROM source_resolutions INDEXED BY source_resolution_running_account WHERE user_id = ? AND outcome IS NULL AND recording_id = ?")
            .bind(actor.id()).bind(recording).fetch_optional(&mut *tx).await?;
        if let Some(id) = pending {
            let admitted = reclaim(&mut tx, &id, token, target).await?;
            receipt_on(&mut tx, actor.id(), request, recording, &id).await?;
            tx.commit().await?;
            return Ok(admitted);
        }
        sqlx::query("UPDATE source_resolutions SET outcome = 'cancelled',completed_at = unixepoch(),lease_token = NULL,lease_until = NULL WHERE outcome IS NULL AND lease_until <= unixepoch()")
            .execute(&mut *tx).await?;
        let (busy, total): (bool, i64) = sqlx::query_as("SELECT EXISTS(SELECT 1 FROM source_resolutions WHERE user_id = ? AND outcome IS NULL),
            (SELECT count(*) FROM (SELECT 1 FROM source_resolutions WHERE outcome IS NULL LIMIT 2))")
            .bind(actor.id()).fetch_one(&mut *tx).await?;
        if busy || total >= 2 {
            tx.commit().await?;
            return Ok(Admission::Busy);
        }
        sqlx::query(
            "INSERT OR IGNORE INTO recording_source_choices(user_id,recording_id) VALUES (?,?)",
        )
        .bind(actor.id())
        .bind(recording)
        .execute(&mut *tx)
        .await?;
        let id = Uuid::new_v4().to_string();
        let target_digest = target.digest()?;
        sqlx::query("INSERT INTO source_resolutions(id,user_id,recording_id,target_digest,choice_revision,lease_token,lease_until) VALUES (?,?,?,?,?,?,unixepoch()+12)")
            .bind(&id).bind(actor.id()).bind(recording).bind(&target_digest).bind(target.choice_revision).bind(token).execute(&mut *tx).await?;
        receipt_on(&mut tx, actor.id(), request, recording, &id).await?;
        tx.commit().await?;
        Ok(Admission::Execute(Box::new(Attempt {
            id,
            token: token.into(),
            target,
            target_digest,
        })))
    }
}

async fn reclaim(
    connection: &mut SqliteConnection,
    id: &str,
    token: &str,
    target: Target,
) -> Result<Admission> {
    let digest = target.digest()?;
    let saved: (String, bool) = sqlx::query_as("SELECT target_digest,lease_until <= unixepoch() FROM source_resolutions WHERE id = ? AND outcome IS NULL")
        .bind(id).fetch_one(&mut *connection).await?;
    if !saved.1 {
        return Ok(Admission::Observe {
            id: id.into(),
            outcome: None,
        });
    }
    if saved.0 != digest {
        finish_on(connection, id, None, SourceResolutionOutcome::Cancelled).await?;
        return Ok(Admission::Observe {
            id: id.into(),
            outcome: Some("cancelled".into()),
        });
    }
    sqlx::query("UPDATE source_resolutions SET lease_token = ?,lease_until = unixepoch()+12 WHERE id = ? AND outcome IS NULL AND lease_until <= unixepoch()")
        .bind(token).bind(id).execute(connection).await?;
    Ok(Admission::Execute(Box::new(Attempt {
        id: id.into(),
        token: token.into(),
        target,
        target_digest: digest,
    })))
}

async fn receipt_on(
    connection: &mut SqliteConnection,
    account: &str,
    request: Uuid,
    recording: &str,
    resolution: &str,
) -> Result<()> {
    sqlx::query("INSERT INTO source_resolution_receipts(user_id,request_id,recording_id,resolution_id) VALUES (?,?,?,?)")
        .bind(account).bind(request.to_string()).bind(recording).bind(resolution).execute(connection).await?;
    Ok(())
}

pub(super) async fn recheck_on(connection: &mut SqliteConnection, attempt: &Attempt) -> Result<()> {
    let current: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM source_resolutions WHERE id = ? AND outcome IS NULL AND lease_token = ? AND lease_until > unixepoch())")
        .bind(&attempt.id).bind(&attempt.token).fetch_one(connection).await?;
    if current {
        Ok(())
    } else {
        Err(Error::Conflict(
            "This source resolution was replaced or expired".into(),
        ))
    }
}

pub(super) async fn finish_on(
    connection: &mut SqliteConnection,
    id: &str,
    token: Option<&str>,
    outcome: SourceResolutionOutcome,
) -> Result<()> {
    let changed = sqlx::query("UPDATE source_resolutions SET outcome = ?,completed_at = unixepoch(),lease_token = NULL,lease_until = NULL WHERE id = ? AND outcome IS NULL AND (? IS NULL OR lease_token = ?)")
        .bind(outcome.as_str()).bind(id).bind(token).bind(token).execute(connection).await?.rows_affected();
    if changed == 1 {
        Ok(())
    } else {
        Err(Error::Conflict(
            "This source resolution was replaced or completed".into(),
        ))
    }
}
