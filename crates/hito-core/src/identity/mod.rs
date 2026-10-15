mod access;
mod accounts;
mod credentials;
mod sessions;
mod setup;
mod types;

use crate::store::Store;
pub(crate) use access::{library_access, recording_access};
pub use types::{Actor, AuthSession, Credentials, Library, User};

#[derive(Clone)]
pub struct Identity {
    pub(crate) store: Store,
}
