use super::{
    ReplicaSession, ReplicaSyncOutcome, Replicas, files,
    transport::{Endpoint, Remote},
};
use crate::{Actor, Error, Operation, Result, SyncBatch};
use zeroize::Zeroizing;

struct Snapshot {
    endpoint: Endpoint,
    token: Zeroizing<String>,
    batch: SyncBatch,
    cursor: i64,
}

impl Replicas {
    async fn snapshot(&self, actor: &Actor, session: &ReplicaSession) -> Result<Snapshot> {
        let mut guard = self.guarded(actor, session).await?;
        let (endpoint, credential): (String, Option<Vec<u8>>) =
            sqlx::query_as("SELECT endpoint,credential FROM replica_connections WHERE id=?")
                .bind(session.connection.to_string())
                .fetch_one(&mut *guard)
                .await?;
        let credential = credential.ok_or(Error::AuthorizationRequired(
            "Sign in to this Hito server again",
        ))?;
        let token = self.vault.reveal_replica(
            actor.id(),
            &session.connection.to_string(),
            &endpoint,
            &session.remote_account,
            &credential,
        )?;
        guard.commit().await?;
        let profile = self
            .profiles
            .active
            .lock()
            .map_err(|_| files::unavailable())?
            .get(&session.connection)
            .cloned()
            .ok_or(Error::NotFound)?;
        let mut tx = profile.store.pool.begin().await?;
        let cursor: i64 = sqlx::query_scalar("SELECT cursor FROM replica_state WHERE singleton=1")
            .fetch_one(&mut *tx)
            .await?;
        let payloads:Vec<String>=sqlx::query_scalar("SELECT payload FROM replica_outbox WHERE echoed=0 AND acknowledged=0 ORDER BY ordinal LIMIT 100").fetch_all(&mut *tx).await?;
        let mut batch = SyncBatch {
            device_id: session.client_id.as_str().to_owned(),
            operations: Vec::new(),
        };
        let mut bytes = serde_json::to_vec(&batch)
            .map_err(|_| files::unavailable())?
            .len();
        for payload in payloads {
            let next = payload.len() + usize::from(!batch.operations.is_empty());
            if next > super::transport::MAX_REQUEST_BYTES.saturating_sub(bytes) {
                if batch.operations.is_empty() {
                    return Err(Error::OperationRejected(
                        "A stored replica operation exceeds the remote protocol limit".into(),
                    ));
                }
                break;
            }
            batch.operations.push(
                serde_json::from_str::<Operation>(&payload)
                    .map_err(|_| Error::Internal("Stored replica operation is invalid".into()))?,
            );
            bytes += next;
        }
        let guard = self.guarded(actor, session).await?;
        tx.commit().await?;
        drop(profile);
        guard.commit().await?;
        Ok(Snapshot {
            endpoint: Endpoint::parse(&endpoint)?,
            token,
            batch,
            cursor,
        })
    }

    #[tracing::instrument(level="info",skip_all,err(Debug),fields(account_id=actor.id(),connection_id=%session.connection,request_id=%uuid::Uuid::new_v4()))]
    pub async fn synchronize(
        &self,
        actor: &Actor,
        session: &ReplicaSession,
    ) -> Result<ReplicaSyncOutcome> {
        let started = std::time::Instant::now();
        let _binding = self
            .profiles
            .synchronizing
            .enter(&session.connection.to_string())
            .map_err(|_| Error::Conflict("This replica is already synchronizing".into()))?;
        let _network = self
            .profiles
            .network
            .clone()
            .try_acquire_owned()
            .map_err(|_| Error::Conflict("Replica requests are busy. Try again.".into()))?;
        let snapshot = self.snapshot(actor, session).await?;
        let remote = Remote::new(snapshot.endpoint)?;
        let pushed = snapshot.batch.operations.len();
        if pushed > 0 {
            let receipt = self
                .remote_session_call(
                    session,
                    remote.push(&snapshot.token, &session.remote_account, &snapshot.batch),
                )
                .await?;
            self.acknowledge(actor, session, &snapshot.batch, &receipt)
                .await?;
        }
        let page = self
            .remote_session_call(
                session,
                remote.pull(&snapshot.token, &session.remote_account, snapshot.cursor),
            )
            .await?;
        let mut outcome = self
            .reconcile(actor, session, snapshot.cursor, page)
            .await?;
        outcome.pushed = pushed;
        tracing::info!(
            outcome = "synchronized",
            pushed = outcome.pushed,
            received = outcome.received,
            pending = outcome.pending,
            elapsed_ms = started.elapsed().as_millis() as u64,
            "Replica synchronization pass completed"
        );
        Ok(outcome)
    }
}
