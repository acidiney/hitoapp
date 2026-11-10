use super::{
    ConnectReplica, ReplicaConnection, ReplicaConnectionState, ReplicaSession, Replicas,
    SyncClientId,
    transport::{Endpoint, Remote, RemoteError},
};
use crate::{Actor, Error, Result};
use uuid::Uuid;
use zeroize::Zeroizing;

#[derive(sqlx::FromRow)]
struct ConnectionRow {
    id: String,
    remote_user: String,
    client_id: String,
    state: String,
    remote_account_id: String,
    generation: i64,
}

impl ConnectionRow {
    fn public(&self) -> Result<ReplicaConnection> {
        Ok(ReplicaConnection {
            id: Uuid::parse_str(&self.id).map_err(|_| invalid())?,
            account: serde_json::from_str(&self.remote_user).map_err(|_| invalid())?,
            client_id: SyncClientId::new(&self.client_id)?,
            state: match self.state.as_str() {
                "initializing" => ReplicaConnectionState::Initializing,
                "ready" => ReplicaConnectionState::Ready,
                "disconnected" => ReplicaConnectionState::Disconnected,
                _ => return Err(invalid()),
            },
        })
    }
}

fn invalid() -> Error {
    Error::Internal("Stored replica identity is invalid".into())
}

pub(super) fn remote_error(error: RemoteError) -> Error {
    match error {
        RemoteError::Unauthorized => {
            Error::AuthorizationRequired("Sign in to this Hito server again")
        }
        RemoteError::Locked => Error::Locked,
        RemoteError::Forbidden => Error::Forbidden,
        RemoteError::Conflict => {
            Error::Conflict("The remote server rejected this operation identity".into())
        }
        RemoteError::RateLimited { retry_after } => {
            Error::RateLimited(retry_after.map_or(1, |delay| delay.as_secs().clamp(1, 3600)))
        }
        RemoteError::Unavailable => Error::Upstream("The Hito server is unavailable"),
        RemoteError::InvalidRequest => Error::OperationRejected(
            "The replica request exceeds the remote protocol limits".into(),
        ),
        RemoteError::InvalidReply => Error::Upstream("The Hito server returned an invalid reply"),
    }
}

impl Replicas {
    pub(super) async fn remote_call<T>(
        &self,
        call: impl std::future::Future<Output = super::transport::RemoteResult<T>>,
    ) -> Result<T> {
        let mut shutdown = self.profiles.shutdown.subscribe();
        if *shutdown.borrow() {
            return Err(Error::Conflict("This replica owner is closed".into()));
        }
        tokio::select! {
            biased;
            _=shutdown.changed()=>Err(Error::Conflict("This replica owner is closed".into())),
            result=call=>result.map_err(remote_error),
        }
    }

    #[tracing::instrument(level="debug", skip_all, err(Debug), fields(account_id=actor.id()))]
    pub async fn connections(&self, actor: &Actor) -> Result<Vec<ReplicaConnection>> {
        let mut guard = self.control(actor).await?;
        let rows: Vec<ConnectionRow> = sqlx::query_as("SELECT id,remote_user,client_id,state,remote_account_id,generation FROM replica_connections WHERE user_id=? ORDER BY id LIMIT 17")
            .bind(actor.id()).fetch_all(&mut *guard).await?;
        if rows.len() > 16 {
            return Err(Error::Conflict("Too many server connections".into()));
        }
        rows.into_iter().map(|row| row.public()).collect()
    }

    #[tracing::instrument(level="info", skip_all, err(Debug), fields(account_id=actor.id(), request_id=%Uuid::new_v4()))]
    pub async fn connect(
        &self,
        actor: &Actor,
        request: ConnectReplica,
    ) -> Result<ReplicaConnection> {
        let started = std::time::Instant::now();
        let _network = self
            .profiles
            .network
            .clone()
            .try_acquire_owned()
            .map_err(|_| Error::Conflict("Replica requests are busy. Try again.".into()))?;
        self.control(actor).await?.commit().await?;
        let endpoint = Endpoint::parse(&request.endpoint)?;
        let remote = Remote::new(endpoint.clone())?;
        let authenticated = self.remote_call(remote.login(request.credentials)).await?;
        let token = Zeroizing::new(authenticated.token);
        let user = self
            .remote_call(remote.me(&token, &authenticated.user.id))
            .await?;
        if user.id != authenticated.user.id || user.id.is_empty() || user.id.len() > 128 {
            return Err(Error::Upstream(
                "The Hito server returned a different account",
            ));
        }
        let mut tx = self.control(actor).await?;
        let existing: Option<(String, String)> = sqlx::query_as("SELECT id,client_id FROM replica_connections WHERE user_id=? AND endpoint=? AND remote_account_id=?")
            .bind(actor.id()).bind(endpoint.as_str()).bind(&user.id).fetch_optional(&mut *tx).await?;
        let (id, client) = match existing {
            Some((id, client)) => (
                Uuid::parse_str(&id).map_err(|_| invalid())?,
                SyncClientId::new(client)?,
            ),
            None => {
                let (owned,total): (i64,i64) = sqlx::query_as("SELECT (SELECT count(*) FROM replica_connections WHERE user_id=?),((SELECT count(*) FROM replica_connections)+(SELECT count(*) FROM erased_replica_profiles))")
                    .bind(actor.id()).fetch_one(&mut *tx).await?;
                if owned >= 16 || total >= 128 {
                    return Err(Error::Conflict(
                        "The server connection limit has been reached".into(),
                    ));
                }
                (
                    Uuid::new_v4(),
                    SyncClientId::new(Uuid::new_v4().to_string())?,
                )
            }
        };
        let credential = self.vault.seal_replica(
            actor.id(),
            &id.to_string(),
            endpoint.as_str(),
            &user.id,
            &token,
        )?;
        let user_json = serde_json::to_string(&user).map_err(|_| invalid())?;
        sqlx::query("INSERT INTO replica_connections(id,user_id,endpoint,remote_account_id,remote_user,client_id,state,credential) VALUES(?,?,?,?,?,?,'initializing',?) ON CONFLICT(id) DO UPDATE SET remote_user=excluded.remote_user,credential=excluded.credential,generation=replica_connections.generation+1,state='initializing'")
            .bind(id.to_string()).bind(actor.id()).bind(endpoint.as_str()).bind(&user.id).bind(user_json).bind(client.as_str()).bind(credential).execute(&mut *tx).await?;
        sqlx::query("DELETE FROM replica_activations WHERE connection_id=?")
            .bind(id.to_string())
            .execute(&mut *tx)
            .await?;
        let generation: i64 =
            sqlx::query_scalar("SELECT generation FROM replica_connections WHERE id=?")
                .bind(id.to_string())
                .fetch_one(&mut *tx)
                .await?;
        tx.commit().await?;
        self.retire_connection(id)?;
        self.join_connection(id).await?;
        let profile = self.profile(id, &user.id, client.as_str()).await?;
        drop(profile);
        let mut tx = self.control(actor).await?;
        let updated = sqlx::query("UPDATE replica_connections SET state='ready' WHERE id=? AND user_id=? AND generation=? AND state='initializing'")
            .bind(id.to_string()).bind(actor.id()).bind(generation).execute(&mut *tx).await?.rows_affected();
        if updated != 1 {
            return Err(Error::Conflict(
                "This connection attempt has been retired".into(),
            ));
        }
        tx.commit().await?;
        tracing::info!(outcome="connected",connection_id=%id,elapsed_ms=started.elapsed().as_millis() as u64,"Remote replica connected");
        Ok(ReplicaConnection {
            id,
            account: user,
            client_id: client,
            state: ReplicaConnectionState::Ready,
        })
    }

    #[tracing::instrument(level="info", skip_all, err(Debug), fields(account_id=actor.id(),connection_id=%connection))]
    pub async fn activate(&self, actor: &Actor, connection: Uuid) -> Result<ReplicaSession> {
        let mut guard = self.control(actor).await?;
        let row: ConnectionRow=sqlx::query_as("SELECT id,remote_user,client_id,state,remote_account_id,generation FROM replica_connections WHERE id=? AND user_id=?")
            .bind(connection.to_string()).bind(actor.id()).fetch_optional(&mut *guard).await?.ok_or(Error::NotFound)?;
        if row.state == "initializing" {
            return Err(Error::Conflict(
                "This connection is still being initialized".into(),
            ));
        }
        guard.commit().await?;
        drop(
            self.profile(connection, &row.remote_account_id, &row.client_id)
                .await?,
        );
        let mut guard = self.control(actor).await?;
        let valid:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM replica_connections WHERE id=? AND user_id=? AND generation=? AND state<>'initializing')")
            .bind(connection.to_string()).bind(actor.id()).bind(row.generation).fetch_one(&mut *guard).await?;
        if !valid {
            return Err(Error::Conflict("This connection has changed".into()));
        }
        let previous: Option<(String, String)> = sqlx::query_as(
            "SELECT connection_id,activation_id FROM replica_activations WHERE session_hash=?",
        )
        .bind(actor.session_binding())
        .fetch_optional(&mut *guard)
        .await?;
        let activation = Uuid::new_v4();
        let signal = self.register_lease(connection, activation)?;
        sqlx::query("INSERT INTO replica_activations(session_hash,connection_id,activation_id) VALUES(?,?,?) ON CONFLICT(session_hash) DO UPDATE SET connection_id=excluded.connection_id,activation_id=excluded.activation_id")
            .bind(actor.session_binding()).bind(connection.to_string()).bind(activation.to_string()).execute(&mut *guard).await?;
        guard.commit().await?;
        if let Some((connection, activation)) = previous {
            let previous_id = Uuid::parse_str(&activation).map_err(|_| invalid())?;
            self.retire_activation(previous_id)?;
            self.join_connection(Uuid::parse_str(&connection).map_err(|_| invalid())?)
                .await?;
        }
        tracing::info!(outcome = "activated", "Replica profile activated");
        Ok(ReplicaSession {
            owner: actor.clone(),
            instance: self.instance,
            signal,
            connection,
            remote_account: row.remote_account_id,
            client_id: SyncClientId::new(row.client_id)?,
            connection_generation: row.generation,
            activation,
        })
    }

    #[tracing::instrument(level="info", skip_all, err(Debug), fields(account_id=actor.id(),connection_id=%session.connection))]
    pub async fn deactivate(&self, actor: &Actor, session: &ReplicaSession) -> Result<()> {
        let mut guard = self.guarded(actor, session).await?;
        sqlx::query("DELETE FROM replica_activations WHERE session_hash=? AND activation_id=?")
            .bind(actor.session_binding())
            .bind(session.activation.to_string())
            .execute(&mut *guard)
            .await?;
        guard.commit().await?;
        self.retire_activation(session.activation)?;
        self.join_connection(session.connection).await?;
        tracing::info!(outcome = "deactivated", "Replica profile deactivated");
        Ok(())
    }

    #[tracing::instrument(level="info", skip_all, err(Debug), fields(account_id=actor.id(),connection_id=%connection))]
    pub async fn disconnect(&self, actor: &Actor, connection: Uuid) -> Result<()> {
        let mut guard = self.control(actor).await?;
        let affected=sqlx::query("UPDATE replica_connections SET state='disconnected',credential=NULL,generation=generation+1 WHERE id=? AND user_id=?")
            .bind(connection.to_string()).bind(actor.id()).execute(&mut *guard).await?.rows_affected();
        if affected != 1 {
            return Err(Error::NotFound);
        }
        sqlx::query("DELETE FROM replica_activations WHERE connection_id=?")
            .bind(connection.to_string())
            .execute(&mut *guard)
            .await?;
        guard.commit().await?;
        self.retire_connection(connection)?;
        self.join_connection(connection).await?;
        tracing::info!(outcome = "disconnected", "Replica connection disconnected");
        Ok(())
    }
}
