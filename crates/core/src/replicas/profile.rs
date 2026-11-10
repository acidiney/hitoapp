use super::{Replicas, files};
use crate::{Error, Result, store::Store};
use futures_util::FutureExt;
use std::sync::Arc;
use tracing::{Instrument, instrument::WithSubscriber};
use uuid::Uuid;

pub(super) struct Profile {
    pub store: Store,
    _files: files::ProfileFiles,
}

impl Replicas {
    pub(super) async fn profile(
        &self,
        id: Uuid,
        remote: &str,
        client: &str,
    ) -> Result<Arc<Profile>> {
        self.ensure_open()?;
        let future = {
            let mut initializers = self
                .profiles
                .initializers
                .lock()
                .map_err(|_| files::unavailable())?;
            initializers
                .entry(id)
                .or_insert_with(|| {
                    let this = self.clone();
                    let remote = remote.to_owned();
                    let client = client.to_owned();
                    let task = tokio::spawn(
                        async move { this.open_profile(id, &remote, &client).await }
                            .instrument(tracing::Span::current())
                            .with_current_subscriber(),
                    );
                    async move {
                        task.await
                            .map_err(|_| Arc::new(files::unavailable()))?
                            .map_err(Arc::new)
                    }
                    .boxed()
                    .shared()
                })
                .clone()
        };
        let result = future.await;
        self.profiles
            .initializers
            .lock()
            .map_err(|_| files::unavailable())?
            .remove(&id);
        result.map_err(|_| files::unavailable())
    }

    async fn open_profile(&self, id: Uuid, remote: &str, client: &str) -> Result<Arc<Profile>> {
        let _opening = self.profiles.opening.lock().await;
        if let Some(profile) = self
            .profiles
            .active
            .lock()
            .map_err(|_| files::unavailable())?
            .get(&id)
            .cloned()
        {
            return Ok(profile);
        }
        self.ensure_open()?;
        let exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM replica_connections WHERE id=? AND remote_account_id=? AND client_id=?)")
            .bind(id.to_string()).bind(remote).bind(client).fetch_one(&self.store.pool).await?;
        if !exists {
            return Err(Error::NotFound);
        }
        let owned = files::open(&self.root, id)?;
        let store = Store::open(&owned.path).await?;
        let mut tx = store.pool.begin_with("BEGIN IMMEDIATE").await?;
        let existing: Option<(String, String, String)> = sqlx::query_as(
            "SELECT binding_id,remote_account_id,client_id FROM replica_state WHERE singleton=1",
        )
        .fetch_optional(&mut *tx)
        .await?;
        if let Some(existing) = existing {
            if existing != (id.to_string(), remote.to_owned(), client.to_owned()) {
                return Err(Error::Conflict(
                    "This replica belongs to another connection".into(),
                ));
            }
        } else {
            sqlx::query("INSERT INTO users(id,username,password_hash,admin) VALUES(?,?,'!',0)")
                .bind(remote)
                .bind(format!("replica-{id}"))
                .execute(&mut *tx)
                .await?;
            sqlx::query("INSERT INTO replica_state(singleton,binding_id,remote_account_id,client_id) VALUES(1,?,?,?)")
                .bind(id.to_string()).bind(remote).bind(client).execute(&mut *tx).await?;
        }
        tx.commit().await?;
        let exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM replica_connections WHERE id=? AND remote_account_id=? AND client_id=?)")
            .bind(id.to_string()).bind(remote).bind(client).fetch_one(&self.store.pool).await?;
        if !exists {
            store.pool.close().await;
            return Err(Error::NotFound);
        }
        let profile = Arc::new(Profile {
            store,
            _files: owned,
        });
        self.profiles
            .active
            .lock()
            .map_err(|_| files::unavailable())?
            .insert(id, profile.clone());
        Ok(profile)
    }

    #[tracing::instrument(level = "info", skip_all, err(Debug))]
    pub async fn close(&self) -> Result<()> {
        self.profiles
            .closed
            .store(true, std::sync::atomic::Ordering::SeqCst);
        self.profiles.shutdown.send_replace(true);
        let _network = self
            .profiles
            .network
            .clone()
            .acquire_many_owned(4)
            .await
            .map_err(|_| files::unavailable())?;
        let initializing: Vec<_> = self
            .profiles
            .initializers
            .lock()
            .map_err(|_| files::unavailable())?
            .drain()
            .map(|(_, future)| future)
            .collect();
        for initialized in initializing {
            let _ = initialized.await;
        }
        let _opening = self.profiles.opening.lock().await;
        let profiles: Vec<_> = self
            .profiles
            .active
            .lock()
            .map_err(|_| files::unavailable())?
            .drain()
            .map(|(_, profile)| profile)
            .collect();
        for profile in profiles {
            profile.store.pool.close().await;
        }
        tracing::info!(outcome = "closed", "Replica owner closed");
        Ok(())
    }
}
