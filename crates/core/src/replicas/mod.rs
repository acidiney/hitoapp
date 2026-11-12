mod authority;
mod catalog;
mod changes;
mod connections;
mod files;
mod lifecycle;
mod profile;
mod projection;
mod reading;
mod reconciliation;
mod retirement;
mod synchronization;
pub(super) mod transport;
mod types;

use crate::{secrets::Vault, store::Store};
use futures_util::future::{BoxFuture, Shared};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, atomic::AtomicBool},
};
use tokio::sync::Mutex as AsyncMutex;
pub use types::{
    ConnectReplica, QueuedReplicaChange, ReplicaConnection, ReplicaConnectionState, ReplicaSession,
    ReplicaSyncOutcome, SyncClientId,
};
use uuid::Uuid;

#[derive(Clone)]
pub struct Replicas {
    pub(super) store: Store,
    pub(super) vault: Vault,
    pub(super) root: Arc<PathBuf>,
    profiles: Arc<Profiles>,
    instance: Uuid,
}

type Initialization =
    Shared<BoxFuture<'static, std::result::Result<Arc<profile::Profile>, Arc<crate::Error>>>>;

struct Profiles {
    active: Mutex<HashMap<Uuid, Arc<profile::Profile>>>,
    initializers: Mutex<HashMap<Uuid, Initialization>>,
    opening: AsyncMutex<()>,
    catalog: Mutex<HashMap<Uuid, catalog::Work>>,
    closed: AtomicBool,
    synchronizing: crate::admission::Accounts,
    network: Arc<tokio::sync::Semaphore>,
    shutdown: tokio::sync::watch::Sender<bool>,
    signals: Mutex<HashMap<Uuid, std::sync::Weak<lifecycle::LeaseSignal>>>,
}

impl Replicas {
    pub(crate) fn new(store: Store, vault: Vault, database: &Path) -> Self {
        Self {
            store,
            vault,
            root: Arc::new(database.with_extension("replicas")),
            profiles: Default::default(),
            instance: Uuid::new_v4(),
        }
    }
}

impl Default for Profiles {
    fn default() -> Self {
        Self {
            active: Default::default(),
            initializers: Default::default(),
            opening: Default::default(),
            catalog: Default::default(),
            closed: AtomicBool::new(false),
            synchronizing: Default::default(),
            network: Arc::new(tokio::sync::Semaphore::new(4)),
            shutdown: tokio::sync::watch::channel(false).0,
            signals: Default::default(),
        }
    }
}
