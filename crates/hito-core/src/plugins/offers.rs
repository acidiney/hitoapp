use super::{OfferPluginService, PluginService, Plugins};
use crate::{Actor, Error, Result};
use url::Url;
use uuid::Uuid;

impl Plugins {
    #[tracing::instrument(level = "info", skip_all, err(Debug), fields(account_id = actor.id()))]
    pub async fn offer_service(
        &self,
        actor: &Actor,
        request: OfferPluginService,
    ) -> Result<PluginService> {
        actor.require_admin()?;
        if request.name.trim().is_empty()
            || request.name.len() > 256
            || request.name.chars().any(char::is_control)
        {
            return Err(Error::Invalid(
                "Use a service name of 1 to 256 bytes without control characters".into(),
            ));
        }
        let base_url = endpoint(&request.base_url)?.to_string();
        let mut tx = self.store.pool.begin_with("BEGIN IMMEDIATE").await?;
        let count: i64 =
            sqlx::query_scalar("SELECT count(*) FROM plugin_services WHERE active = 1")
                .fetch_one(&mut *tx)
                .await?;
        if count >= 64 {
            return Err(Error::Invalid(
                "This server already offers 64 plugin services".into(),
            ));
        }
        let exists: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM plugin_services WHERE base_url = ? AND active = 1)",
        )
        .bind(&base_url)
        .fetch_one(&mut *tx)
        .await?;
        if exists {
            return Err(Error::Conflict("This service is already offered".into()));
        }
        let service = PluginService {
            id: Uuid::new_v4().to_string(),
            name: request.name,
            base_url,
            revision: 1,
        };
        sqlx::query("INSERT INTO plugin_services(id,name,base_url) VALUES (?,?,?)")
            .bind(&service.id)
            .bind(&service.name)
            .bind(&service.base_url)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        tracing::info!(service_id = service.id, "Plugin service offered");
        Ok(service)
    }
}

pub(super) fn endpoint(value: &str) -> Result<Url> {
    let invalid = || {
        Error::Invalid(
            "Use an HTTP or HTTPS service address without credentials, query, or fragment".into(),
        )
    };
    if value.len() > 2048 || value.chars().any(char::is_control) {
        return Err(invalid());
    }
    let mut url = Url::parse(value.trim()).map_err(|_| invalid())?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(invalid());
    }
    if !url.path().ends_with('/') {
        url.set_path(&format!("{}/", url.path()));
    }
    Ok(url)
}
