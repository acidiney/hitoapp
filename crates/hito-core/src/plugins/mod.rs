mod authorization;
mod credentials;
mod download;
mod installations;
mod key_file;
mod media;
mod offers;
mod operation;
mod playlist_operation;
mod playlists;
mod preparation;
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
pub(crate) use download::ProviderDownloadOrigin;
pub(crate) use media::Authority as AudioAuthority;
pub(crate) use operation::Operation;
pub(crate) use playlist_operation::PlaylistOperation;
pub(crate) use playlists::PlaylistSnapshot;
pub(crate) use preparation::{PreparationConnection, PreparationExchange, PreparationOperation};
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
