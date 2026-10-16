mod catalog;
mod collection;
mod context;
mod error;
mod identity;
mod library_storage;
mod metadata;
mod plugins;
mod store;

pub use catalog::{
    AdoptProviderRecording, Artist, ArtistDetail, Artwork, AudioContent, AudioFile, AudioRequest,
    AudioStream, Browse, ByteRange, Catalog, OpenedAudio, Page, PlaybackCandidates, PlaybackSource,
    ProviderSearch, Recording, Release, ReleaseDetail, ReleaseTrack, Search, Source,
};
pub use collection::{
    Action, Activity, Collection, HistoryEntry, Operation, Playlist, PlaylistDetail, PlaylistEntry,
    SavedPosition, SyncBatch, SyncEvent, SyncPage, SyncReceipt,
};
pub use context::{CoreConfig, HostDeviceId, MediaTools, SharedDirectoryConfig, StorageConfig};
pub use error::{Error, Result};
pub use identity::{Actor, AuthSession, Credentials, Identity, Library, User};
pub use library_storage::{
    BrowseDirectories, ConnectFolder, DirectoryEntry, DirectoryPage, DirectoryRef, Job,
    LibraryStorage, MusicFolder, SharedDirectory,
};
pub use metadata::{
    InspectedTags, MetadataHistoryQuery, MetadataObservation, ObservationOrigin, ObservationPage,
    ObservedTag, RecordingMetadata, ReleaseMetadata, TagLocation, TextMetadata,
};
pub use plugins::{
    InstallPlugin, OfferPluginService, PluginCredential, PluginInstallation, PluginRefresh,
    PluginService, PluginSettings, Plugins, RefreshPlugin,
};
use store::Store;

#[derive(Clone)]
pub struct Services {
    pub identity: Identity,
    pub catalog: Catalog,
    pub collection: Collection,
    pub storage: LibraryStorage,
    pub plugins: Plugins,
    store: Store,
}

#[tracing::instrument(level = "info", skip_all, err(Debug), fields(host_device = config.host_device.as_str()))]
pub async fn open(config: CoreConfig) -> Result<Services> {
    let directories = library_storage::Directories::new(config.storage)?;
    let store = Store::open(&config.database).await?;
    let storage = LibraryStorage {
        store: store.clone(),
        host: config.host_device.clone(),
        directories,
        inspector: library_storage::FileInspector::new(config.media_tools),
    };
    storage.initialize().await?;
    let plugins = Plugins::new(store.clone(), &config.database).await?;
    tracing::info!("Core services ready");
    Ok(Services {
        identity: Identity {
            store: store.clone(),
        },
        catalog: Catalog {
            plugins: plugins.clone(),
            store: store.clone(),
            host: config.host_device.clone(),
            artwork_slots: std::sync::Arc::new(tokio::sync::Semaphore::new(4)),
            directories: storage.directories.clone(),
        },
        collection: Collection {
            store: store.clone(),
            host: config.host_device.clone(),
        },
        storage,
        plugins,
        store,
    })
}

impl Services {
    pub async fn healthy(&self) -> bool {
        self.store.healthy().await
    }
}
