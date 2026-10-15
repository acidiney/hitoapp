use super::{InstallPlugin, PluginInstallation, PluginService, PluginSettings, Plugins};
use crate::{Actor, Error, Result};
use sqlx::Row;
use uuid::Uuid;

impl Plugins {
    #[tracing::instrument(level = "debug", skip_all, err(Debug), fields(account_id = actor.id()))]
    pub async fn settings(&self, actor: &Actor) -> Result<PluginSettings> {
        let mut tx = self.store.pool.begin().await?;
        let services = sqlx::query_as::<_, PluginService>("SELECT id,name,base_url,revision FROM plugin_services WHERE active = 1 ORDER BY name COLLATE NOCASE,id")
            .fetch_all(&mut *tx).await?;
        let rows = sqlx::query("SELECT id,service_id,manifest,revision,checked_at FROM plugin_installations WHERE user_id = ? AND active = 1 ORDER BY checked_at DESC,id")
            .bind(actor.id()).fetch_all(&mut *tx).await?;
        let installations = rows
            .into_iter()
            .map(installation)
            .collect::<Result<Vec<_>>>()?;
        tx.commit().await?;
        tracing::debug!(
            count = installations.len(),
            "Personal plugin settings loaded"
        );
        Ok(PluginSettings {
            services,
            installations,
        })
    }

    #[tracing::instrument(level = "info", skip_all, err(Debug), fields(account_id = actor.id()))]
    pub async fn install(
        &self,
        actor: &Actor,
        request: InstallPlugin,
    ) -> Result<PluginInstallation> {
        let service: PluginService = sqlx::query_as(
            "SELECT id,name,base_url,revision FROM plugin_services WHERE id = ? AND active = 1",
        )
        .bind(&request.service_id)
        .fetch_optional(&self.store.pool)
        .await?
        .ok_or(Error::NotFound)?;
        if service.revision != request.service_revision {
            return Err(Error::Conflict(
                "This plugin service changed. Reload Settings.".into(),
            ));
        }
        let manifest = self.remote.manifest(&service.base_url).await?;
        let id = Uuid::new_v4().to_string();
        let mut tx = self.store.pool.begin_with("BEGIN IMMEDIATE").await?;
        let exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM plugin_installations WHERE user_id = ? AND service_id = ? AND active = 1)")
            .bind(actor.id()).bind(&service.id).fetch_one(&mut *tx).await?;
        if exists {
            return Err(Error::Conflict(
                "This plugin is already connected to your account".into(),
            ));
        }
        let count: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM plugin_installations WHERE user_id = ? AND active = 1",
        )
        .bind(actor.id())
        .fetch_one(&mut *tx)
        .await?;
        if count >= 32 {
            return Err(Error::Invalid(
                "Your account already has 32 connected plugins".into(),
            ));
        }
        let row = sqlx::query("INSERT INTO plugin_installations(id,user_id,service_id,manifest)
            SELECT ?,?,?,? WHERE EXISTS(SELECT 1 FROM plugin_services WHERE id = ? AND active = 1 AND revision = ?)
            RETURNING id,service_id,manifest,revision,checked_at")
            .bind(&id).bind(actor.id()).bind(&service.id).bind(serde_json::to_string(&manifest).map_err(|_| Error::Internal("Plugin manifest could not be stored".into()))?)
            .bind(&service.id).bind(service.revision).fetch_optional(&mut *tx).await?
            .ok_or_else(|| Error::Conflict("This plugin service was withdrawn during connection".into()))?;
        let installed = installation(row)?;
        tx.commit().await?;
        tracing::info!(
            installation_id = id,
            service_id = service.id,
            "Personal plugin connected"
        );
        Ok(installed)
    }
}

fn installation(row: sqlx::sqlite::SqliteRow) -> Result<PluginInstallation> {
    let manifest: String = row.try_get("manifest")?;
    Ok(PluginInstallation {
        id: row.try_get("id")?,
        service_id: row.try_get("service_id")?,
        manifest: serde_json::from_str(&manifest)
            .map_err(|_| Error::Internal("Stored plugin manifest is invalid".into()))?,
        revision: row.try_get("revision")?,
        checked_at: row.try_get("checked_at")?,
    })
}
