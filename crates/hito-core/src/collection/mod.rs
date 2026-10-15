mod apply;
mod events;
mod history;
mod playlists;
mod service;
mod types;

use crate::{HostDeviceId, store::Store};
pub use types::{
    Action, Activity, HistoryEntry, Operation, Playlist, PlaylistDetail, PlaylistEntry, SyncBatch,
    SyncEvent, SyncPage, SyncReceipt,
};

#[derive(Clone)]
pub struct Collection {
    pub(crate) store: Store,
    pub(crate) host: HostDeviceId,
}
