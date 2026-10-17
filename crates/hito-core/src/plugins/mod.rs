mod admission;
mod authorization;
mod credentials;
mod installations;
mod key_file;
mod media;
mod offers;
mod operation;
mod playlists;
mod refresh;
mod removal;
mod transport;
mod types;
mod vault;

use crate::{Result, store::Store};
pub use authorization::{
    PluginAuthorization, PluginAuthorizationRedirect, PluginAuthorizationState,
    PluginAuthorizationStatus,
};
pub(crate) use media::{Authority as AudioAuthority, RemoteAudio};
pub(crate) use operation::Operation;
pub(crate) use playlists::PlaylistSnapshot;
pub(crate) use transport::SourceBatch;
pub(crate) use transport::artwork::{MAX_ARTWORK_BYTES, ProviderArtwork};
pub use types::{
    InstallPlugin, OfferPluginService, PluginCredential, PluginInstallation, PluginRefresh,
    PluginService, PluginSettings, RefreshPlugin,
};

#[derive(Clone)]
pub struct Plugins {
    store: Store,
    remote: transport::ControlClient,
    media: media::MediaClient,
    vault: vault::Vault,
}

impl Plugins {
    pub(crate) async fn new(store: Store, database: &std::path::Path) -> Result<Self> {
        let vault = vault::Vault::open(&store, database).await?;
        Ok(Self {
            store,
            vault,
            remote: transport::ControlClient::new()?,
            media: media::MediaClient::new()?,
        })
    }
}
