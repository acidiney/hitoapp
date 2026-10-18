mod apply;
mod audiobooks;
mod events;
mod follow;
mod history;
mod import;
mod import_persist;
mod import_publication;
mod import_receipts;
mod metadata_refresh;
mod playlist_entries;
mod playlists;
mod saved_playback;
mod saved_position;
mod service;
mod types;

#[cfg(test)]
#[path = "tests/audiobook_admission_work.rs"]
mod audiobook_admission_work;
#[cfg(test)]
#[path = "tests/saved_playback_work.rs"]
mod saved_playback_work;

use crate::{HostDeviceId, store::Store};
pub use audiobooks::{
    AudiobookBookmark, AudiobookChapterAnchor, AudiobookContinuation, AudiobookDetail,
};
pub use follow::{
    FollowProviderPlaylist, PlaylistFollow, PlaylistFollowInterval, PlaylistFollowReceipt,
    PlaylistFollowRefresh, PlaylistFollowStatus, RefreshFollowedPlaylist,
};
pub use import::ImportProviderPlaylist;
pub use import_publication::{
    CollectionPublication, ImportedPlaylistEntry, ImportedPlaylistEntryKind,
    ImportedPlaylistEntryOrigin, ImportedPlaylistOrigin, ImportedPlaylistRecording,
    ImportedPlaylistSnapshot, PlaylistMetadataRefreshOrigin,
};
pub use import_receipts::PlaylistImportReceipt;
pub use metadata_refresh::{ImportedPlaylistMetadataRefresh, RefreshImportedPlaylistMetadata};
pub use types::{
    Action, Activity, HistoryEntry, Operation, Playlist, PlaylistDetail, PlaylistEntry,
    RecordingProgress, SavedPosition, SyncBatch, SyncEvent, SyncPage, SyncReceipt,
};

#[derive(Clone)]
pub struct Collection {
    pub(crate) store: Store,
    pub(crate) host: HostDeviceId,
    pub(crate) plugins: crate::Plugins,
    pub(crate) metadata_refresh_admission: metadata_refresh::Admission,
}
