use super::Plugins;
use crate::{Actor, Error, Result};
use hito_plugin_sdk::{Capability, Manifest, SearchPage, SearchRequest};
use sqlx::Row;
use zeroize::Zeroizing;

pub(crate) struct Operation {
    account: String,
    installation: String,
    installation_revision: i64,
    service: String,
    service_revision: i64,
    base_url: String,
    token: Option<Zeroizing<String>>,
    remote: super::transport::ControlClient,
}
impl Plugins {
    #[tracing::instrument(level = "debug", skip_all, err(Debug), fields(account_id = actor.id()))]
    pub(crate) async fn operation(
        &self,
        actor: &Actor,
        id: &str,
        expected_revision: i64,
        capabilities: &[Capability],
    ) -> Result<Operation> {
        let row = sqlx::query("SELECT p.revision,p.service_id,p.manifest,p.credential,s.revision AS service_revision,s.base_url
            FROM plugin_installations p JOIN plugin_services s ON s.id = p.service_id AND s.active = 1
            WHERE p.id = ? AND p.user_id = ? AND p.active = 1")
            .bind(id).bind(actor.id()).fetch_optional(&self.store.pool).await?.ok_or(Error::NotFound)?;
        let revision: i64 = row.try_get("revision")?;
        if revision != expected_revision {
            return Err(Error::Conflict(
                "This plugin connection changed. Reload it before continuing.".into(),
            ));
        }
        let manifest: Manifest = serde_json::from_str(row.try_get("manifest")?)
            .map_err(|_| Error::Internal("Stored plugin manifest is invalid".into()))?;
        if !capabilities
            .iter()
            .all(|capability| manifest.capabilities.contains(capability))
        {
            return Err(Error::Invalid(
                "This plugin has not been approved for the requested capability".into(),
            ));
        }
        let service: String = row.try_get("service_id")?;
        let credential: Option<Vec<u8>> = row.try_get("credential")?;
        let token = credential
            .as_deref()
            .map(|blob| self.vault.reveal(actor.id(), id, &service, blob))
            .transpose()?;
        Ok(Operation {
            account: actor.id().into(),
            installation: id.into(),
            installation_revision: revision,
            service,
            service_revision: row.try_get("service_revision")?,
            base_url: row.try_get("base_url")?,
            token,
            remote: self.remote.clone(),
        })
    }
}
impl Operation {
    pub(crate) async fn search(&self, request: &SearchRequest) -> Result<SearchPage> {
        self.remote
            .search(
                &self.account,
                &self.base_url,
                self.token.as_deref().map(String::as_str),
                request,
            )
            .await
    }
    pub(crate) async fn recheck(&self, connection: &mut sqlx::SqliteConnection) -> Result<()> {
        let current: Option<(i64, i64)> = sqlx::query_as("SELECT p.revision,s.revision FROM plugin_installations p JOIN plugin_services s ON s.id = p.service_id AND s.active = 1
            WHERE p.id = ? AND p.user_id = ? AND p.service_id = ? AND p.active = 1")
            .bind(&self.installation).bind(&self.account).bind(&self.service).fetch_optional(connection).await?;
        let current = current.ok_or(Error::NotFound)?;
        if current != (self.installation_revision, self.service_revision) {
            return Err(Error::Conflict(
                "The plugin connection changed during the request. Try again.".into(),
            ));
        }
        Ok(())
    }
}
