mod catalog;
mod collection;
mod context;
mod error;
mod identity;
mod indexing;
mod media_file;
mod store;

pub use catalog::{Artwork, Catalog, Page, Recording, Search, Source};
pub use collection::{
    Action, Activity, Collection, HistoryEntry, Operation, Playlist, PlaylistDetail, PlaylistEntry,
    SyncBatch, SyncEvent, SyncPage, SyncReceipt,
};
pub use context::{CoreConfig, HostDeviceId, MediaTools};
pub use error::{Error, Result};
pub use identity::{Actor, AuthSession, Credentials, Identity, Library, User};
pub use indexing::{Indexer, Job};
use store::Store;

#[derive(Clone)]
pub struct Services {
    pub identity: Identity,
    pub catalog: Catalog,
    pub collection: Collection,
    pub indexer: Indexer,
    store: Store,
}

#[tracing::instrument(level = "info", skip_all, err(Debug), fields(host_device = config.host_device.as_str()))]
pub async fn open(config: CoreConfig) -> Result<Services> {
    let store = Store::open(&config.database).await?;
    tracing::info!("Core services ready");
    Ok(Services {
        identity: Identity {
            store: store.clone(),
        },
        catalog: Catalog {
            store: store.clone(),
            host: config.host_device.clone(),
            artwork_slots: std::sync::Arc::new(tokio::sync::Semaphore::new(4)),
        },
        collection: Collection {
            store: store.clone(),
            host: config.host_device.clone(),
        },
        indexer: Indexer {
            store: store.clone(),
            host: config.host_device,
            music_root: config.music_root,
            inspector: indexing::FileInspector::new(config.media_tools),
        },
        store,
    })
}

impl Services {
    pub async fn healthy(&self) -> bool {
        self.store.healthy().await
    }
}
