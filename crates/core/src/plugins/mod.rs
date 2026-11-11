mod authorization;
mod configured;
mod credentials;
mod download;
mod health;
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
    PluginAuthorization, PluginAuthorizationCallback, PluginAuthorizationOutcome,
    PluginAuthorizationRedirect, PluginAuthorizationState, PluginAuthorizationStatus,
    PluginConnectionState, PluginConnectionStatus, ProviderAccess,
};
pub use configured::{ConfiguredOffer, ConfiguredPluginService};
pub(crate) use download::ProviderDownloadOrigin;
pub(crate) use media::Authority as AudioAuthority;
pub(crate) use operation::{LyricsDocumentResponse, Operation};
pub(crate) use playlist_operation::{PlaylistOperation, PlaylistOperationFailure};
pub(crate) use playlists::PlaylistSnapshot;
pub(crate) use preparation::{PreparationConnection, PreparationExchange, PreparationOperation};
pub(crate) use transport::SourceBatch;
pub(crate) use transport::artwork::{MAX_ARTWORK_BYTES, ProviderArtwork};
pub use types::{
    Connections, InstallPlugin, OfferPluginService, PluginCredential, PluginDeclarationCheck,
    PluginHealth, PluginHealthState, PluginInstallation, PluginInstallationPreview, PluginRefresh,
    PluginService, PluginSettings, PreviewPluginInstallation, RefreshPlugin,
};

#[derive(Clone)]
pub struct Plugins {
    store: Store,
    remote: transport::ControlClient,
    media: media::MediaClient,
    vault: vault::Vault,
    token: authorization::TokenClient,
    connection_locks: std::sync::Arc<authorization::ConnectionLocks>,
    health: health::Cache,
}

impl Plugins {
    pub(crate) async fn new(
        store: Store,
        vault: vault::Vault,
        pace: crate::StreamPace,
    ) -> Result<Self> {
        Ok(Self {
            store,
            vault,
            remote: transport::ControlClient::new()?,
            media: media::MediaClient::new(pace)?,
            token: authorization::TokenClient::new()?,
            connection_locks: std::sync::Arc::default(),
            health: health::Cache::default(),
        })
    }
}
