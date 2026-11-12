use super::{ReplicaSession, Replicas};
use crate::{Actor, Error, Result};
use sqlx::{Sqlite, Transaction};
use std::sync::atomic::Ordering;

impl Replicas {
    pub(super) fn ensure_open(&self) -> Result<()> {
        if self.profiles.closed.load(Ordering::SeqCst) {
            return Err(Error::Conflict("This replica owner is closed".into()));
        }
        Ok(())
    }

    pub(super) async fn control(&self, actor: &Actor) -> Result<Transaction<'_, Sqlite>> {
        self.ensure_open()?;
        actor.require_session()?;
        actor.require_unlocked()?;
        let mut tx = self.store.pool.begin_with("BEGIN IMMEDIATE").await?;
        actor.check_session_on(&mut tx).await?;
        self.ensure_open()?;
        Ok(tx)
    }

    pub(super) async fn guarded(
        &self,
        actor: &Actor,
        session: &ReplicaSession,
    ) -> Result<Transaction<'_, Sqlite>> {
        if self.instance != session.instance || *session.signal.stopped.borrow() {
            return Err(Error::Conflict(
                "This replica session has been retired".into(),
            ));
        }
        if !actor.same_session(&session.owner) {
            return Err(Error::Forbidden);
        }
        let mut tx = self.control(actor).await?;
        let valid: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM replica_connections c JOIN replica_activations a ON a.connection_id=c.id WHERE c.id=? AND c.user_id=? AND c.remote_account_id=? AND c.client_id=? AND c.generation=? AND c.state<>'initializing' AND a.session_hash=? AND a.activation_id=?)")
            .bind(session.connection.to_string()).bind(actor.id()).bind(&session.remote_account).bind(session.client_id.as_str()).bind(session.connection_generation)
            .bind(actor.session_binding()).bind(session.activation.to_string()).fetch_one(&mut *tx).await?;
        if !valid {
            return Err(Error::Conflict(
                "This replica session has been retired".into(),
            ));
        }
        Ok(tx)
    }
}
