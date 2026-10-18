mod authorization;
mod credentials;
mod download;
mod install_preview;
mod installations;
mod manifest;
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
use crate::secrets as vault;

use crate::{Result, store::Store};
pub use authorization::{
    PluginAuthorization, PluginAuthorizationRedirect, PluginAuthorizationState,
    PluginAuthorizationStatus,
};
pub(crate) use download::ProviderDownloadOrigin;
pub(crate) use media::Authority as AudioAuthority;
pub(crate) use operation::Operation;
pub(crate) use playlist_operation::{PlaylistOperation, PlaylistOperationFailure};
pub(crate) use playlists::PlaylistSnapshot;
pub(crate) use preparation::{PreparationConnection, PreparationExchange, PreparationOperation};
pub(crate) use transport::SourceBatch;
pub(crate) use transport::artwork::{MAX_ARTWORK_BYTES, ProviderArtwork};
pub use types::{
    InstallPlugin, OfferPluginService, PluginCredential, PluginInstallation,
    PluginInstallationPreview, PluginRefresh, PluginService, PluginSettings,
    PreviewPluginInstallation, RefreshPlugin,
};

#[derive(Clone)]
pub struct Plugins {
    store: Store,
    remote: transport::ControlClient,
    media: media::MediaClient,
    vault: vault::Vault,
}

impl Plugins {
    pub(crate) async fn new(store: Store, vault: vault::Vault) -> Result<Self> {
        Ok(Self {
            store,
            vault,
            remote: transport::ControlClient::new()?,
            media: media::MediaClient::new()?,
        })
    }
}
