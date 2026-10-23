use super::{arguments::Arguments, private_files};
use hito_debrid_premiumize::{MediaAllowlist, PremiumizeClient, TransferFileBinding};
use hito_plugin_premiumize::{Config, ConnectionConfig, CuratedWavRecording};
use hito_plugin_sdk::ProviderRecording;
use serde::Deserialize;
use std::net::SocketAddr;

const MAX_CATALOG_BYTES: usize = 1_048_576;

pub(super) struct Settings {
    pub listen: SocketAddr,
    pub provider: Config,
}

impl Settings {
    pub(super) fn read() -> Result<Self, &'static str> {
        let arguments = Arguments::read()?;
        let mut provider_token = private_files::read_token(&arguments.provider)?;
        let mut service_token = private_files::read_token(&arguments.service)?;
        if provider_token.as_str() == service_token.as_str() {
            return Err("credentials_must_differ");
        }
        let receipt_key = private_files::read_receipt_key(&arguments.receipt)?;
        let bytes = private_files::read(&arguments.catalog, MAX_CATALOG_BYTES)?;
        let catalog: Catalog =
            serde_json::from_slice(&bytes).map_err(|_| "catalog_file_invalid")?;
        let media_origins =
            MediaAllowlist::new(catalog.media_origins).map_err(|_| "media_origins_invalid")?;
        let recordings = catalog
            .recordings
            .into_iter()
            .map(|entry| CuratedWavRecording {
                recording: entry.recording,
                source_id: entry.source_id,
                label: entry.label,
                binding: entry.binding,
            })
            .collect();
        let client = PremiumizeClient::new(std::mem::take(&mut *provider_token))
            .map_err(|_| "provider_initialization_failed")?;
        Ok(Self {
            listen: arguments.listen,
            provider: Config {
                connections: vec![ConnectionConfig {
                    service_bearer: std::mem::take(&mut *service_token),
                    receipt_key: *receipt_key,
                    client,
                    media_origins,
                    recordings,
                }],
            },
        })
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Catalog {
    media_origins: Vec<String>,
    recordings: Vec<Recording>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Recording {
    recording: ProviderRecording,
    source_id: String,
    label: String,
    binding: TransferFileBinding,
}
