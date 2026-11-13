mod access;
mod backend;
mod browser;
mod folders;
mod host;
mod loopback;
mod media_tools;
#[cfg(feature = "gui")]
mod native_picker;
#[cfg(feature = "native-media")]
mod os_controls;
mod storage;
mod system_media;
mod user_agent;

pub use browser::SystemBrowser;
pub use folders::{FolderPicker, HostChoice, MusicLookup, music_folder, proposal};
pub use host::LocalHost;
pub use loopback::{DEFAULT_RETURN_PORT, InvalidReturnPort, RETURN_PORT_SETTING, ReturnPort};
pub use media_tools::MediaToolSearch;
#[cfg(feature = "gui")]
pub use native_picker::NativePicker;
#[cfg(feature = "native-media")]
pub use os_controls::{OsControls, WindowHandle};
use std::path::PathBuf;
pub use system_media::{Controls, Create, Lazy, Metadata, SystemMedia};
pub use user_agent::user_agent;

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
