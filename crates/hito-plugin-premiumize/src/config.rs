use hito_debrid_premiumize::{MediaAllowlist, PremiumizeClient, TransferFileBinding};
use hito_plugin_sdk::ProviderRecording;
use std::sync::Arc;
use zeroize::{Zeroize, Zeroizing};

use crate::{catalog::Catalog, policy, receipts::Receipts};

pub struct Config {
    pub connections: Vec<ConnectionConfig>,
}

pub struct ConnectionConfig {
    pub service_bearer: String,
    /// Rotate this key whenever the configured provider account changes.
    pub receipt_key: [u8; 32],
    pub client: PremiumizeClient,
    pub media_origins: MediaAllowlist,
    pub recordings: Vec<CuratedWavRecording>,
}

pub struct CuratedWavRecording {
    pub recording: ProviderRecording,
    pub source_id: String,
    pub label: String,
    pub binding: TransferFileBinding,
}

pub(crate) struct Connection {
    pub(crate) service_authority: [u8; 32],
    pub(crate) client: PremiumizeClient,
    pub(crate) media_origins: MediaAllowlist,
    pub(crate) catalog: Catalog,
    pub(crate) receipts: Receipts,
}

pub(crate) fn connections(config: Config) -> Result<Vec<Arc<Connection>>, InitError> {
    if config.connections.is_empty() || config.connections.len() > 16 {
        return Err(InitError::InvalidConfiguration);
    }
    let mut connections: Vec<Arc<Connection>> = Vec::with_capacity(config.connections.len());
    for mut config in config.connections {
        let service_bearer = Zeroizing::new(std::mem::take(&mut config.service_bearer));
        let service_authority =
            policy::credential_digest(&service_bearer).ok_or(InitError::InvalidConfiguration)?;
        let receipt_key = Zeroizing::new(config.receipt_key);
        config.receipt_key.zeroize();
        if receipt_key.iter().all(|byte| *byte == 0)
            || connections.iter().any(|connection| {
                connection.service_authority == service_authority
                    || connection.receipts.same_key(&receipt_key)
            })
        {
            return Err(InitError::InvalidConfiguration);
        }
        let catalog = Catalog::new(config.recordings)?;
        let receipts = Receipts::new(receipt_key, service_authority, catalog.fingerprint());
        connections.push(Arc::new(Connection {
            service_authority,
            client: config.client,
            media_origins: config.media_origins,
            catalog,
            receipts,
        }));
    }
    Ok(connections)
}

#[derive(Debug, thiserror::Error)]
pub enum InitError {
    #[error("The Premiumize plugin configuration is invalid")]
    InvalidConfiguration,
}
