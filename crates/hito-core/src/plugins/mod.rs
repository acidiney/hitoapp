mod installations;
mod offers;
mod removal;
mod transport;
mod types;

use crate::{Result, store::Store};
pub use types::{
    InstallPlugin, OfferPluginService, PluginInstallation, PluginService, PluginSettings,
};

#[derive(Clone)]
pub struct Plugins {
    store: Store,
    remote: transport::ManifestClient,
}

impl Plugins {
    pub(crate) fn new(store: Store) -> Result<Self> {
        Ok(Self {
            store,
            remote: transport::ManifestClient::new()?,
        })
    }
}
