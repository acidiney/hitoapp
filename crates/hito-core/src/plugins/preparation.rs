use super::Plugins;
use crate::{Error, Result};
use hito_plugin_sdk::{Capability, Manifest};
use sqlx::{Row, SqliteConnection};

mod operation;
pub(crate) use operation::{PreparationExchange, PreparationOperation};

pub(crate) struct PreparationConnection {
    pub installation_id: String,
    pub installation_revision: i64,
    pub service_id: String,
    pub service_revision: i64,
    pub provider: String,
    pub credential: Option<Vec<u8>>,
    pub audio_generation: i64,
}

impl Plugins {
    pub(crate) async fn preparation_connection_on(
        &self,
        connection: &mut SqliteConnection,
        account: &str,
        installation: &str,
    ) -> Result<PreparationConnection> {
        let row = sqlx::query("SELECT installation.id,installation.revision,installation.service_id,
            service.revision AS service_revision,installation.manifest,installation.credential,installation.audio_generation
            FROM plugin_installations installation JOIN plugin_services service
                ON service.id = installation.service_id AND service.active = 1
            WHERE installation.id = ? AND installation.user_id = ? AND installation.active = 1")
            .bind(installation).bind(account).fetch_optional(connection).await?.ok_or(Error::NotFound)?;
        let manifest: Manifest = serde_json::from_str(row.try_get("manifest")?)
            .map_err(|_| Error::Internal("Stored plugin manifest is invalid".into()))?;
        if !manifest.capabilities.contains(&Capability::Sources)
            || !manifest
                .capabilities
                .contains(&Capability::SourcePreparation)
        {
            return Err(Error::Invalid(
                "This plugin connection is not approved for source preparation".into(),
            ));
        }
        Ok(PreparationConnection {
            installation_id: row.try_get("id")?,
            installation_revision: row.try_get("revision")?,
            service_id: row.try_get("service_id")?,
            service_revision: row.try_get("service_revision")?,
            provider: manifest.id,
            credential: row.try_get("credential")?,
            audio_generation: row.try_get("audio_generation")?,
        })
    }
}
