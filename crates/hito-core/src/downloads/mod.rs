mod cleanup;
mod health;
mod lease;
mod playlist_changes;
mod playlist_reconcile;
mod playlist_requests;
mod playlist_status;
mod playlist_types;
mod publication;
mod requests;
mod retention;
mod retirement;
mod roots;
mod types;
mod worker;

use crate::{HostDeviceId, Plugins, library_storage::ManagedFiles, store::Store};
pub(crate) use health::retire_unusable_copy;
pub(crate) use playlist_changes::playlist_changed_on;
pub use playlist_types::{
    PlaylistDownloadEntries, PlaylistDownloadEntry, PlaylistDownloadEntryState,
    PlaylistDownloadQuery, PlaylistDownloadReceipt, PlaylistDownloadStatus,
};
pub use types::{DownloadReceipt, DownloadRequest, DownloadState, DownloadStatus};

#[derive(Clone)]
pub struct Downloads {
    pub(crate) store: Store,
    pub(crate) plugins: Plugins,
    pub(crate) host: HostDeviceId,
    pub(crate) files: ManagedFiles,
    pub(crate) active: active::ActiveJobs,
    pub(crate) playlist_turn: std::sync::Arc<std::sync::atomic::AtomicUsize>,
}
mod active;
