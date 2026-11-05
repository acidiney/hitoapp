use super::{Plugins, offers::endpoint};
use crate::{Error, Result};
use std::time::Instant;
use uuid::Uuid;

const MAX_CONFIGURED: usize = 16;
const MAX_LABEL: usize = 32;
const STARTUP_REQUESTER: &str = "configured-plugin-services";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConfiguredPluginService {
    label: String,
    base_url: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConfiguredOffer {
    Offered,
    AlreadyKnown,
}

impl ConfiguredPluginService {
    pub fn parse_list(value: &str) -> Result<Vec<Self>> {
        let mut services: Vec<Self> = Vec::new();
        for entry in value.split(',').map(str::trim).filter(|e| !e.is_empty()) {
            let service = Self::parse(entry)?;
            if services
                .iter()
                .any(|known| known.label == service.label || known.base_url == service.base_url)
            {
                return Err(Error::Invalid(
                    "Each configured plugin service needs its own label and address".into(),
                ));
            }
            services.push(service);
        }
        if services.len() > MAX_CONFIGURED {
            return Err(Error::Invalid(format!(
                "At most {MAX_CONFIGURED} plugin services can be configured"
            )));
        }
        Ok(services)
    }

    fn parse(entry: &str) -> Result<Self> {
        let invalid = || {
            Error::Invalid(
                "Configure each plugin service as label=address, for example spotify=http://plugin-spotify:8092/"
                    .into(),
            )
        };
        let (label, address) = entry.split_once('=').ok_or_else(invalid)?;
        let label = label.trim();
        if label.is_empty()
            || label.len() > MAX_LABEL
            || !label
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        {
            return Err(invalid());
        }
        Ok(Self {
            label: label.into(),
            base_url: endpoint(address)?.to_string(),
        })
    }

    pub fn label(&self) -> &str {
        &self.label
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }
}

impl Plugins {
    #[tracing::instrument(level = "info", skip_all)]
    pub async fn offer_configured(
        &self,
        service: &ConfiguredPluginService,
    ) -> Result<ConfiguredOffer> {
        let started = Instant::now();
        let result = self.offer_configured_inner(service).await;
        let elapsed_ms = started.elapsed().as_millis() as u64;
        match &result {
            Ok(ConfiguredOffer::Offered) => tracing::info!(
                label = service.label(),
                elapsed_ms,
                outcome = "offered",
                "Configured plugin service offered"
            ),
            Ok(ConfiguredOffer::AlreadyKnown) => tracing::debug!(
                label = service.label(),
                elapsed_ms,
                outcome = "already_known",
                "Configured plugin service already known"
            ),
            Err(error) => tracing::warn!(
                label = service.label(),
                elapsed_ms,
                outcome = error.code(),
                "Configured plugin service not offered"
            ),
        }
        result
    }

    async fn offer_configured_inner(
        &self,
        service: &ConfiguredPluginService,
    ) -> Result<ConfiguredOffer> {
        if self.known_address(service.base_url()).await? {
            return Ok(ConfiguredOffer::AlreadyKnown);
        }
        let manifest = self
            .remote
            .manifest(STARTUP_REQUESTER, service.base_url(), None)
            .await?;
        let mut tx = self.store.pool.begin_with("BEGIN IMMEDIATE").await?;
        let known: bool =
            sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM plugin_services WHERE base_url = ?)")
                .bind(service.base_url())
                .fetch_one(&mut *tx)
                .await?;
        if known {
            return Ok(ConfiguredOffer::AlreadyKnown);
        }
        let active: i64 =
            sqlx::query_scalar("SELECT count(*) FROM plugin_services WHERE active = 1")
                .fetch_one(&mut *tx)
                .await?;
        if active >= 64 {
            return Err(Error::Invalid(
                "This server already offers 64 plugin services".into(),
            ));
        }
        sqlx::query("INSERT INTO plugin_services(id,name,base_url) VALUES (?,?,?)")
            .bind(Uuid::new_v4().to_string())
            .bind(manifest.name)
            .bind(service.base_url())
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(ConfiguredOffer::Offered)
    }

    async fn known_address(&self, base_url: &str) -> Result<bool> {
        Ok(
            sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM plugin_services WHERE base_url = ?)")
                .bind(base_url)
                .fetch_one(&self.store.pool)
                .await?,
        )
    }
}
