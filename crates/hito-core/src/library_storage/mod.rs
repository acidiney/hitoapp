mod directories;
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
pub(crate) use directories::Directories;
pub use directories::SharedDirectory;
pub(crate) use inspection::FileInspector;
pub use types::Job;

#[derive(Clone)]
pub struct LibraryStorage {
    pub(crate) store: Store,
    pub(crate) host: HostDeviceId,
    pub(crate) directories: Directories,
    pub(crate) inspector: FileInspector,
}
