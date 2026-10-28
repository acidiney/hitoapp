mod access;
mod accounts;
mod credentials;
mod locks;
mod pins;
mod sessions;
mod setup;
mod types;

use crate::store::Store;
pub(crate) use access::{library_access, personal_library, recording_access, shared_library};
pub use types::{
    Actor, AuthSession, Credentials, Library, LockStatus, RemovePin, SetPin, Unlock, User,
};

#[derive(Clone)]
pub struct Identity {
    pub(crate) store: Store,
}
