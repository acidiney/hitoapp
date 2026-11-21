mod cleanup;
mod guard;
mod health;
mod lease;
mod managed_origin;
mod origin;
mod playlist_admission;
mod playlist_changes;
mod playlist_reconcile;
mod playlist_requests;
mod playlist_retry;
mod playlist_status;
mod playlist_types;
mod podcast;
mod publication;
mod requests;
mod retention;
mod retirement;
mod roots;
mod transfer;
mod types;
mod worker;

use crate::{HostDeviceId, Plugins, library_storage::ManagedFiles, store::Store};
use health::retire_unusable_copy;
pub(crate) use managed_origin::ManagedOrigin;
pub(crate) use origin::OriginSelection;
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
    pub(crate) remote: crate::remote_http::RemoteHttp,
    pub(crate) host: HostDeviceId,
    pub(crate) files: ManagedFiles,
    pub(crate) active: active::ActiveJobs,
    pub(crate) playlist_turn: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    pub(crate) authority_interval: std::time::Duration,
}

pub(crate) const AUTHORITY_INTERVAL: std::time::Duration = std::time::Duration::from_millis(250);

impl Downloads {
    #[cfg(any(test, feature = "test-support"))]
    pub fn with_authority_interval(mut self, interval: std::time::Duration) -> Self {
        self.authority_interval = interval;
        self
    }
}
mod active;
