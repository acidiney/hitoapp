use super::{Replicas, files};
use crate::Result;
use uuid::Uuid;

impl Replicas {
    #[tracing::instrument(level = "debug", skip_all, err(Debug))]
    pub(crate) async fn retire_pending(&self) -> Result<()> {
        let queued: Vec<String> =
            sqlx::query_scalar("SELECT id FROM erased_replica_profiles ORDER BY id LIMIT 128")
                .fetch_all(&self.store.pool)
                .await?;
        let mut first_error = None;
        let mut bindings = Vec::with_capacity(queued.len());
        for id in queued {
            match Uuid::parse_str(&id) {
                Ok(binding) => {
                    if let Err(error) = self.retire_connection(binding) {
                        first_error.get_or_insert(error);
                    }
                    bindings.push((id, binding));
                }
                Err(_) => {
                    tracing::warn!(
                        outcome = "invalid_record",
                        "Replica retirement remains pending"
                    );
                    first_error.get_or_insert_with(files::unavailable);
                }
            }
        }
        for (id, binding) in bindings {
            if let Err(error) = self.retire_profile(&id, binding).await {
                tracing::warn!(connection_id=%binding,outcome="pending",error_code=error.code(),"Replica file retirement will be retried");
                first_error.get_or_insert(error);
            }
        }
        match first_error {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }

    async fn retire_profile(&self, id: &str, binding: Uuid) -> Result<()> {
        self.join_connection(binding).await?;
        let initializing = self
            .profiles
            .initializers
            .lock()
            .map_err(|_| files::unavailable())?
            .remove(&binding);
        if let Some(initializing) = initializing {
            let _ = initializing.await;
        }
        let _opening = self.profiles.opening.lock().await;
        let profile = self
            .profiles
            .active
            .lock()
            .map_err(|_| files::unavailable())?
            .remove(&binding);
        if let Some(profile) = profile {
            profile.store.pool.close().await;
            drop(profile);
        }
        files::remove(&self.root, binding)?;
        sqlx::query("DELETE FROM erased_replica_profiles WHERE id=?")
            .bind(id)
            .execute(&self.store.pool)
            .await?;
        tracing::info!(connection_id=%binding,outcome="erased","Retired replica files removed");
        Ok(())
    }
}
