mod access;
mod host;
mod storage;

pub use host::LocalHost;
use std::path::PathBuf;

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
