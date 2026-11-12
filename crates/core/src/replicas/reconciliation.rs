use super::{ReplicaSession, ReplicaSyncOutcome, Replicas, files, projection};
use crate::{Actor, Error, Result, SyncBatch, SyncEvent, SyncPage, SyncReceipt};
use std::collections::HashSet;

fn invalid() -> Error {
    Error::Upstream("The Hito server returned an invalid synchronization page")
}

fn validate_page(after: i64, page: &SyncPage) -> Result<()> {
    if page.operations.len() > 200
        || page.cursor < after
        || (page.has_more && page.operations.is_empty())
    {
        return Err(invalid());
    }
    let mut cursor = after;
    let mut ids = HashSet::new();
    for event in &page.operations {
        if event.sequence() <= cursor {
            return Err(invalid());
        }
        let SyncEvent::Operation {
            device_id,
            operation,
            ..
        } = event
        else {
            return Err(Error::OperationRejected(
                "This replica cannot project server publications yet".into(),
            ));
        };
        super::SyncClientId::new(device_id).map_err(|_| invalid())?;
        projection::playlist(operation)?;
        if !ids.insert(operation.id) {
            return Err(invalid());
        }
        cursor = event.sequence();
    }
    if cursor != page.cursor
        || serde_json::to_vec(page).map_err(|_| invalid())?.len() > 16 * 1024 * 1024
    {
        return Err(invalid());
    }
    Ok(())
}

impl Replicas {
    pub(super) async fn acknowledge(
        &self,
        actor: &Actor,
        session: &ReplicaSession,
        batch: &SyncBatch,
        receipt: &SyncReceipt,
    ) -> Result<()> {
        let expected: HashSet<_> = batch
            .operations
            .iter()
            .map(|operation| operation.id)
            .collect();
        let acknowledged: HashSet<_> = receipt.acknowledged.iter().copied().collect();
        if expected != acknowledged || receipt.acknowledged.len() != batch.operations.len() {
            return Err(Error::Upstream(
                "The Hito server did not acknowledge the exact replica batch",
            ));
        }
        self.guarded(actor, session).await?.commit().await?;
        let profile = self
            .profiles
            .active
            .lock()
            .map_err(|_| files::unavailable())?
            .get(&session.connection)
            .cloned()
            .ok_or(Error::NotFound)?;
        let mut tx = profile.store.pool.begin_with("BEGIN IMMEDIATE").await?;
        for operation in &batch.operations {
            sqlx::query("UPDATE replica_outbox SET acknowledged=1 WHERE operation_id=?")
                .bind(operation.id.to_string())
                .execute(&mut *tx)
                .await?;
        }
        let guard = self.guarded(actor, session).await?;
        tx.commit().await?;
        drop(profile);
        guard.commit().await?;
        Ok(())
    }

    pub(super) async fn reconcile(
        &self,
        actor: &Actor,
        session: &ReplicaSession,
        after: i64,
        page: SyncPage,
    ) -> Result<ReplicaSyncOutcome> {
        validate_page(after, &page)?;
        self.guarded(actor, session).await?.commit().await?;
        let profile = self
            .profiles
            .active
            .lock()
            .map_err(|_| files::unavailable())?
            .get(&session.connection)
            .cloned()
            .ok_or(Error::NotFound)?;
        let mut tx = profile.store.pool.begin_with("BEGIN IMMEDIATE").await?;
        let cursor: i64 = sqlx::query_scalar("SELECT cursor FROM replica_state WHERE singleton=1")
            .fetch_one(&mut *tx)
            .await?;
        if cursor != after {
            return Err(Error::Conflict("The replica cursor has changed".into()));
        }
        let retained: i64 = sqlx::query_scalar("SELECT count(*) FROM replica_received_events")
            .fetch_one(&mut *tx)
            .await?;
        if retained + page.operations.len() as i64 > 100_000 {
            return Err(Error::Conflict(
                "Replica synchronization history is full".into(),
            ));
        }
        let mut changed = HashSet::new();
        for event in &page.operations {
            let SyncEvent::Operation {
                sequence,
                device_id,
                operation,
            } = event
            else {
                return Err(invalid());
            };
            let payload = serde_json::to_string(operation).map_err(|_| invalid())?;
            let previous: Option<(String, String)> = sqlx::query_as(
                "SELECT client_id,payload FROM replica_received_events WHERE operation_id=?",
            )
            .bind(operation.id.to_string())
            .fetch_optional(&mut *tx)
            .await?;
            if previous.is_some() {
                return Err(invalid());
            }
            let pending: Option<String> =
                sqlx::query_scalar("SELECT payload FROM replica_outbox WHERE operation_id=?")
                    .bind(operation.id.to_string())
                    .fetch_optional(&mut *tx)
                    .await?;
            if let Some(pending) = pending {
                if device_id != session.client_id.as_str() || pending != payload {
                    return Err(invalid());
                }
                sqlx::query(
                    "UPDATE replica_outbox SET echoed=1,acknowledged=1 WHERE operation_id=?",
                )
                .bind(operation.id.to_string())
                .execute(&mut *tx)
                .await?;
            }
            changed.insert(projection::accept(&mut tx, operation).await?);
            sqlx::query("INSERT INTO replica_received_events(sequence,operation_id,client_id,payload) VALUES(?,?,?,?)")
                .bind(sequence).bind(operation.id.to_string()).bind(device_id).bind(payload).execute(&mut *tx).await?;
        }
        for playlist in changed {
            projection::rebuild(&mut tx, &session.remote_account, playlist).await?;
        }
        let pending: i64 = sqlx::query_scalar("SELECT count(*) FROM replica_outbox WHERE echoed=0")
            .fetch_one(&mut *tx)
            .await?;
        let awaiting: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM replica_outbox WHERE echoed=0 AND acknowledged=1",
        )
        .fetch_one(&mut *tx)
        .await?;
        if !page.has_more && awaiting > 0 {
            return Err(invalid());
        }
        sqlx::query("UPDATE replica_state SET cursor=? WHERE singleton=1")
            .bind(page.cursor)
            .execute(&mut *tx)
            .await?;
        let guard = self.guarded(actor, session).await?;
        tx.commit().await?;
        drop(profile);
        guard.commit().await?;
        Ok(ReplicaSyncOutcome {
            pushed: 0,
            received: page.operations.len(),
            cursor: page.cursor,
            pending: pending as usize,
            has_more: page.has_more || pending > 0,
        })
    }
}
