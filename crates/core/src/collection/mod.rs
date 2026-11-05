mod apply;
mod audiobooks;
mod events;
mod follow;
mod history;
mod history_privacy;
mod home;
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
#[path = "tests/audiobook_library_work.rs"]
mod audiobook_library_work;
#[cfg(test)]
#[path = "tests/home_work.rs"]
mod home_work;
#[cfg(test)]
#[path = "tests/saved_playback_work.rs"]
mod saved_playback_work;
#[cfg(test)]
#[path = "tests/saved_progress_deadline.rs"]
mod saved_progress_deadline;

use crate::{HostDeviceId, store::Store};
pub use audiobooks::{
    AudiobookBookmark, AudiobookChapterAnchor, AudiobookContinuation, AudiobookDetail,
    AudiobookLibraryPage, AudiobookLibraryQuery, AudiobookPagination, AudiobookPlayback,
    AudiobookPlaybackAdmission,
};
pub use follow::{
    FollowProviderPlaylist, PlaylistFollow, PlaylistFollowInterval, PlaylistFollowReceipt,
    PlaylistFollowRefresh, PlaylistFollowStatus, RefreshFollowedPlaylist,
};
pub use import::ImportProviderPlaylist;
pub(crate) use import::existing_copy;
pub use import_publication::{
    CollectionPublication, ImportedPlaylistEntry, ImportedPlaylistEntryKind,
    ImportedPlaylistEntryOrigin, ImportedPlaylistOrigin, ImportedPlaylistRecording,
    ImportedPlaylistSnapshot, PlaylistMetadataRefreshOrigin,
};
pub use import_receipts::{OpenedProviderPlaylist, PlaylistImportReceipt};
pub use metadata_refresh::{ImportedPlaylistMetadataRefresh, RefreshImportedPlaylistMetadata};
pub(crate) use saved_playback::saved_progress;
pub use types::{
    Action, Activity, AddedAlbum, HistoryCleared, HistoryEntry, HistoryPreference, Home, HomeAlbum,
    Operation, Playlist, PlaylistDetail, PlaylistEntry, RecordingProgress, SavedPosition,
    SyncBatch, SyncEvent, SyncPage, SyncReceipt, UnheardOnAlbum, UpdateHistoryPreference,
};

#[derive(Clone)]
pub struct Collection {
    pub(crate) store: Store,
    pub(crate) host: HostDeviceId,
    pub(crate) plugins: crate::Plugins,
    pub(crate) metadata_refresh_admission: metadata_refresh::Admission,
}
