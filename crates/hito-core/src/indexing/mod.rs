mod inspection;
mod legacy;
mod observations;
mod persistence;
mod resolution;
mod root;
mod scanner;
mod service;
mod types;
mod walking;
mod worker;

use crate::{HostDeviceId, store::Store};
pub(crate) use inspection::FileInspector;
use std::path::PathBuf;
pub use types::Job;

#[derive(Clone)]
pub struct Indexer {
    pub(crate) store: Store,
    pub(crate) host: HostDeviceId,
    pub(crate) music_root: PathBuf,
    pub(crate) inspector: FileInspector,
}
