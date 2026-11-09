use super::{
    Connections, InstallPlugin, PluginInstallation, PluginService, PluginSettings, Plugins,
    manifest::fingerprint,
};
use crate::{Actor, Error, Result};
use sqlx::Row;
use std::time::Instant;
use uuid::Uuid;

impl Plugins {
    #[tracing::instrument(level = "debug", skip_all, err(Debug), fields(account_id = actor.id()))]
    pub async fn settings(&self, actor: &Actor) -> Result<PluginSettings> {
        let Connections {
            services,
            mut installations,
        } = self.connections(actor).await?;
        self.attach_health(actor, &mut installations).await;
        Ok(PluginSettings {
            credential_storage_ready: self.vault.ready(),
            services,
            installations,
        })
    }

    #[tracing::instrument(level = "debug", skip_all, err(Debug), fields(account_id = actor.id()))]
    pub async fn connections(&self, actor: &Actor) -> Result<Connections> {
        let mut tx = self.store.pool.begin().await?;
        let services = sqlx::query_as::<_, PluginService>("SELECT id,name,base_url,revision FROM plugin_services WHERE active = 1 ORDER BY name COLLATE NOCASE,id")
            .fetch_all(&mut *tx).await?;
        let rows = sqlx::query("SELECT id,service_id,manifest,revision,checked_at,credential IS NOT NULL AS credential_set FROM plugin_installations WHERE user_id = ? AND active = 1 ORDER BY checked_at DESC,rowid DESC")
            .bind(actor.id()).fetch_all(&mut *tx).await?;
        let installations = rows
            .into_iter()
            .map(installation)
            .collect::<Result<Vec<_>>>()?;
        tx.commit().await?;
        tracing::debug!(
            service_count = services.len(),
            installation_count = installations.len(),
            "Personal plugin connections loaded"
        );
        Ok(Connections {
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
        let started = Instant::now();
        actor.check_session(&self.store).await?;
        let service = {
            let mut connection = self.store.pool.acquire().await?;
            let service = offered_service(
                &mut connection,
                &request.service_id,
                request.service_revision,
            )
            .await?;
            check_capacity(&mut connection, actor.id(), &service.id).await?;
            service
        };
        let manifest = self
            .remote
            .manifest(
                actor.id(),
                &service.base_url,
                request.bearer_token.as_deref(),
            )
            .await?;
        actor.check_session(&self.store).await?;
        let (encoded, digest) = fingerprint(&manifest)?;
        if request
            .accept_manifest_digest
            .as_ref()
            .is_some_and(|accepted| accepted != &digest)
        {
            tracing::info!(
                service_id = service.id,
                outcome = "manifest_changed",
                elapsed_ms = started.elapsed().as_millis() as u64,
                "Plugin installation rejected"
            );
            return Err(Error::Conflict(
                "The plugin changed after review. Preview the connection again.".into(),
            ));
        }
        if manifest.content_policy.is_some() && request.accept_manifest_digest.is_none() {
            tracing::info!(
                service_id = service.id,
                outcome = "review_required",
                elapsed_ms = started.elapsed().as_millis() as u64,
                "Plugin installation rejected"
            );
            return Err(Error::Conflict(
                "The plugin declares a persistence policy. Review its manifest before connecting."
                    .into(),
            ));
        }
        let id = Uuid::new_v4().to_string();
        let credential = request
            .bearer_token
            .as_deref()
            .map(|token| self.vault.seal(actor.id(), &id, &service.id, token))
            .transpose()?;
        let mut tx = self.store.pool.begin_with("BEGIN IMMEDIATE").await?;
        actor.check_session_on(&mut tx).await?;
        check_capacity(&mut tx, actor.id(), &service.id).await?;
        let row = sqlx::query("INSERT INTO plugin_installations(id,user_id,service_id,manifest,credential)
            SELECT ?,?,?,?,? WHERE EXISTS(SELECT 1 FROM plugin_services WHERE id = ? AND active = 1 AND revision = ?)
            RETURNING id,service_id,manifest,revision,checked_at,credential IS NOT NULL AS credential_set")
            .bind(&id).bind(actor.id()).bind(&service.id).bind(encoded)
            .bind(credential).bind(&service.id).bind(service.revision).fetch_optional(&mut *tx).await?
            .ok_or_else(|| Error::Conflict("This plugin service was withdrawn during connection".into()))?;
        let installed = installation(row)?;
        tx.commit().await?;
        tracing::info!(
            installation_id = id,
            service_id = service.id,
            outcome = "connected",
            elapsed_ms = started.elapsed().as_millis() as u64,
            "Personal plugin connected"
        );
        Ok(installed)
    }
}

pub(super) fn installation(row: sqlx::sqlite::SqliteRow) -> Result<PluginInstallation> {
    let manifest: String = row.try_get("manifest")?;
    Ok(PluginInstallation {
        id: row.try_get("id")?,
        service_id: row.try_get("service_id")?,
        manifest: serde_json::from_str(&manifest)
            .map_err(|_| Error::Internal("Stored plugin manifest is invalid".into()))?,
        revision: row.try_get("revision")?,
        checked_at: row.try_get("checked_at")?,
        credential_set: row.try_get("credential_set")?,
        health: None,
    })
}

pub(super) async fn offered_service(
    connection: &mut sqlx::SqliteConnection,
    id: &str,
    expected_revision: i64,
) -> Result<PluginService> {
    let service: PluginService = sqlx::query_as(
        "SELECT id,name,base_url,revision FROM plugin_services WHERE id = ? AND active = 1",
    )
    .bind(id)
    .fetch_optional(connection)
    .await?
    .ok_or(Error::NotFound)?;
    if service.revision != expected_revision {
        return Err(Error::Conflict(
            "This plugin service changed. Reload Settings.".into(),
        ));
    }
    Ok(service)
}

pub(super) async fn check_capacity(
    connection: &mut sqlx::SqliteConnection,
    account: &str,
    service: &str,
) -> Result<()> {
    let exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM plugin_installations WHERE user_id = ? AND service_id = ? AND active = 1)")
            .bind(account).bind(service).fetch_one(&mut *connection).await?;
    if exists {
        return Err(Error::Conflict(
            "This plugin is already connected to your account".into(),
        ));
    }
    let count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM plugin_installations WHERE user_id = ? AND active = 1",
    )
    .bind(account)
    .fetch_one(&mut *connection)
    .await?;
    if count >= 32 {
        return Err(Error::Invalid(
            "Your account already has 32 connected plugins".into(),
        ));
    }
    Ok(())
}
