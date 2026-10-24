mod audio_file;
mod blocking;
mod browsing;
mod directories;
mod folder_types;
mod folders;
mod initialization;
mod inspection;
mod lease;
mod legacy;
pub(crate) mod local_path;
mod managed;
mod metadata_owner;
mod observations;
mod persistence;
mod resolution;
mod root;
mod scanner;
mod service;
mod types;
mod walking;
mod watching;
mod worker;

use crate::{HostDeviceId, store::Store};
pub use audio_file::AudioFile;
pub use browsing::{BrowseDirectories, DirectoryEntry, DirectoryPage};
pub(crate) use directories::Directories;
pub(crate) use directories::OpenedMedia;
pub use directories::SharedDirectory;
pub use folder_types::{ConnectFolder, DirectoryRef, MusicFolder};
pub(crate) use inspection::FileInspector;
pub(crate) use managed::managed_mime;
pub(crate) use managed::{MAX_MANAGED_BYTES, ManagedAttempt, ManagedFileLocation, ManagedFiles};
pub(crate) use root::Root;
pub use types::Job;
pub use watching::{
    FolderWatchSettings, WatchBackend, WatchCounters, WatchMode, WatchStatus, WatchedFolder,
};

#[derive(Clone)]
pub struct LibraryStorage {
    pub(crate) store: Store,
    pub(crate) host: HostDeviceId,
    pub(crate) directories: Directories,
    pub(crate) inspector: FileInspector,
    pub(crate) watching: FolderWatchSettings,
}
