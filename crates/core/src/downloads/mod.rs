mod cleanup;
mod copies;
mod eviction;
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
mod prefetch;
mod publication;
mod requests;
mod retention;
mod retirement;
mod roots;
mod transfer;
mod types;
mod upcoming;
mod worker;

use crate::{Catalog, Error, HostDeviceId, Plugins, library_storage::ManagedFiles, store::Store};
pub use copies::{AutomaticCopies, AutomaticCopyBudget, AutomaticCopyPreference};
use health::retire_unusable_copy;
pub(crate) use managed_origin::ManagedOrigin;
pub(crate) use origin::OriginSelection;
pub(crate) use playlist_changes::playlist_changed_on;
pub use playlist_types::{
    PlaylistDownloadEntries, PlaylistDownloadEntry, PlaylistDownloadEntryState,
    PlaylistDownloadQuery, PlaylistDownloadReceipt, PlaylistDownloadStatus,
};
pub(crate) use prefetch::request_copy_after_play_on;
pub use types::{DownloadReceipt, DownloadRequest, DownloadState, DownloadStatus};
pub(crate) use upcoming::AnnouncementPace;
pub use upcoming::{MAX_UPCOMING_RECORDINGS, UpcomingRecordings};

#[derive(Clone)]
pub struct Downloads {
    pub(crate) store: Store,
    pub(crate) plugins: Plugins,
    pub(crate) catalog: Catalog,
    pub(crate) remote: crate::remote_http::RemoteHttp,
    pub(crate) host: HostDeviceId,
    pub(crate) files: ManagedFiles,
    pub(crate) active: active::ActiveJobs,
    pub(crate) playlist_turn: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    pub(crate) authority_interval: std::time::Duration,
    pub(crate) announcements: AnnouncementPace,
}

pub(crate) const AUTHORITY_INTERVAL: std::time::Duration = std::time::Duration::from_millis(250);
const BACKGROUND_YIELDED: &str = "Background audio yielded to playback";

pub(crate) fn yielded() -> Error {
    Error::Conflict(BACKGROUND_YIELDED.into())
}

pub(crate) fn has_yielded(error: &Error) -> bool {
    matches!(error, Error::Conflict(message) if message == BACKGROUND_YIELDED)
}

impl Downloads {
    #[cfg(any(test, feature = "test-support"))]
    pub fn with_authority_interval(mut self, interval: std::time::Duration) -> Self {
        self.authority_interval = interval;
        self
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn simulate_disk_space(&self, space: Option<crate::DiskSpace>) {
        self.files.simulate_disk_space(space);
    }
}
mod active;
