mod apply;
mod events;
mod follow;
mod history;
mod import;
mod import_persist;
mod import_publication;
mod import_receipts;
mod playlist_entries;
mod playlists;
mod saved_position;
mod service;
mod types;

use crate::{HostDeviceId, store::Store};
pub use follow::{
    FollowProviderPlaylist, PlaylistFollow, PlaylistFollowInterval, PlaylistFollowReceipt,
    PlaylistFollowRefresh, PlaylistFollowStatus, RefreshFollowedPlaylist,
};
pub use import::ImportProviderPlaylist;
pub use import_publication::{
    CollectionPublication, ImportedPlaylistEntry, ImportedPlaylistEntryKind,
    ImportedPlaylistEntryOrigin, ImportedPlaylistOrigin, ImportedPlaylistRecording,
    ImportedPlaylistSnapshot,
};
pub use import_receipts::PlaylistImportReceipt;
pub use types::{
    Action, Activity, HistoryEntry, Operation, Playlist, PlaylistDetail, PlaylistEntry,
    SavedPosition, SyncBatch, SyncEvent, SyncPage, SyncReceipt,
};

#[derive(Clone)]
pub struct Collection {
    pub(crate) store: Store,
    pub(crate) host: HostDeviceId,
    pub(crate) plugins: crate::Plugins,
}
