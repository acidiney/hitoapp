use hito_plugin_sdk::Manifest;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct OfferPluginService {
    pub name: String,
    pub base_url: String,
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct InstallPlugin {
    pub service_id: String,
    pub service_revision: i64,
}

#[derive(Serialize, ToSchema, sqlx::FromRow)]
pub struct PluginService {
    pub id: String,
    pub name: String,
    pub base_url: String,
    pub revision: i64,
}

#[derive(Serialize, ToSchema)]
pub struct PluginInstallation {
    pub id: String,
    pub service_id: String,
    pub manifest: Manifest,
    pub revision: i64,
    pub checked_at: i64,
}

#[derive(Serialize, ToSchema)]
pub struct PluginSettings {
    pub services: Vec<PluginService>,
    pub installations: Vec<PluginInstallation>,
}
