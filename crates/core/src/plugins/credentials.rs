use super::{PluginInstallation, Plugins, installations::installation};
use crate::{Actor, Error, Result};

impl Plugins {
    #[tracing::instrument(level = "info", skip_all, err(Debug), fields(account_id = actor.id()))]
    pub async fn clear_credential(
        &self,
        actor: &Actor,
        id: &str,
        expected_revision: i64,
    ) -> Result<PluginInstallation> {
        let mut tx = self.store.pool.begin_with("BEGIN IMMEDIATE").await?;
        let revision: i64 = sqlx::query_scalar(
            "SELECT revision FROM plugin_installations WHERE id = ? AND user_id = ? AND active = 1",
        )
        .bind(id)
        .bind(actor.id())
        .fetch_optional(&mut *tx)
        .await?
        .ok_or(Error::NotFound)?;
        if revision != expected_revision {
            return Err(Error::Conflict(
                "This plugin connection changed. Reload Settings.".into(),
            ));
        }
        let row = sqlx::query("UPDATE plugin_installations SET credential = NULL, revision = revision + 1 WHERE id = ? AND user_id = ? AND active = 1 AND revision = ? RETURNING id,service_id,manifest,revision,checked_at,credential IS NOT NULL AS credential_set")
            .bind(id).bind(actor.id()).bind(revision).fetch_one(&mut *tx).await?;
        let installation = installation(row)?;
        tx.commit().await?;
        tracing::info!(installation_id = id, "Personal plugin credential cleared");
        Ok(installation)
    }
}
