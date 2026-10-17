mod cleanup;
mod health;
mod lease;
mod publication;
mod requests;
mod retention;
mod types;
mod worker;

use crate::{HostDeviceId, Plugins, library_storage::ManagedFiles, store::Store};
pub(crate) use health::retire_unusable_copy;
pub use types::{DownloadReceipt, DownloadRequest, DownloadState, DownloadStatus};

#[derive(Clone)]
pub struct Downloads {
    pub(crate) store: Store,
    pub(crate) plugins: Plugins,
    pub(crate) host: HostDeviceId,
    pub(crate) files: ManagedFiles,
    pub(crate) active: active::ActiveJobs,
}
mod active;
