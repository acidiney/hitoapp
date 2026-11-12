use super::{QueuedReplicaChange, ReplicaSession, Replicas, files, projection};
use crate::{Actor, Error, Operation, Result};

pub(super) const MAX_LOCAL_OPERATIONS: i64 = 10_000;

impl Replicas {
    #[tracing::instrument(level="info",skip_all,err(Debug),fields(account_id=actor.id(),connection_id=%session.connection,operation_id=%operation.id))]
    pub async fn enqueue(
        &self,
        actor: &Actor,
        session: &ReplicaSession,
        operation: Operation,
    ) -> Result<QueuedReplicaChange> {
        let started = std::time::Instant::now();
        let playlist = projection::playlist(&operation)?;
        let payload = serde_json::to_string(&operation)
            .map_err(|_| Error::Internal("Replica change could not be encoded".into()))?;
        if payload.len() > 4096 {
            return Err(Error::Invalid(
                "The replica operation exceeds its size limit".into(),
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
        let previous: Option<String> =
            sqlx::query_scalar("SELECT payload FROM replica_outbox WHERE operation_id=?")
                .bind(operation.id.to_string())
                .fetch_optional(&mut *tx)
                .await?;
        if let Some(previous) = previous {
            if previous != payload {
                return Err(Error::Conflict(
                    "A replica operation ID was reused with different contents".into(),
                ));
            }
        } else {
            let count: i64 = sqlx::query_scalar("SELECT count(*) FROM replica_outbox")
                .fetch_one(&mut *tx)
                .await?;
            if count >= MAX_LOCAL_OPERATIONS {
                return Err(Error::Conflict(
                    "Replica operation history is full; synchronization maintenance is required"
                        .into(),
                ));
            }
            sqlx::query(
                "INSERT INTO replica_outbox(operation_id,playlist_id,payload) VALUES(?,?,?)",
            )
            .bind(operation.id.to_string())
            .bind(playlist.to_string())
            .bind(payload)
            .execute(&mut *tx)
            .await?;
            projection::append(&mut tx, &session.remote_account, &operation).await?;
        }
        let guard = self.guarded(actor, session).await?;
        tx.commit().await?;
        drop(profile);
        guard.commit().await?;
        tracing::info!(
            outcome = "queued",
            elapsed_ms = started.elapsed().as_millis() as u64,
            "Replica change retained"
        );
        Ok(QueuedReplicaChange {
            operation_id: operation.id,
        })
    }
}
