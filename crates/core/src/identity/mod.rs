mod access;
mod account_data;
mod accounts;
mod credentials;
mod deletion;
mod locks;
mod pins;
mod sessions;
mod setup;
mod types;

use crate::store::Store;
pub(crate) use access::{library_access, personal_library, recording_access, shared_library};
pub use types::{
    AccountDeletion, Actor, AuthSession, Credentials, DeleteAccount, Library, LockStatus,
    RemovePin, SetPin, Unlock, User,
};

#[derive(Clone)]
pub struct Identity {
    pub(crate) store: Store,
}
