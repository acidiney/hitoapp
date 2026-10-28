mod access;
mod backend;
mod folders;
mod host;
#[cfg(feature = "gui")]
mod native_picker;
#[cfg(feature = "gui")]
mod os_controls;
mod storage;
mod system_media;

pub use folders::{FolderPicker, MusicLookup, music_folder, proposal};
pub use host::LocalHost;
#[cfg(feature = "gui")]
pub use native_picker::NativePicker;
#[cfg(feature = "gui")]
pub use os_controls::{OsControls, WindowHandle};
use std::path::PathBuf;
pub use system_media::{Controls, Create, Lazy, Metadata, SystemMedia};

pub struct Paths {
    pub data: PathBuf,
    pub music: PathBuf,
    pub web: PathBuf,
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Hito is already running for this local library.")]
    AlreadyRunning,
    #[error("Hito's private application storage could not be opened.")]
    Storage,
    #[error("The local Hito backend could not start.")]
    Startup,
    #[error("The local Hito backend did not shut down cleanly.")]
    Shutdown,
}

pub type Result<T> = std::result::Result<T, Error>;
