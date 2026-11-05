mod access;
mod account_data;
mod accounts;
mod credentials;
mod deletion;
mod display_name;
mod locks;
mod open_sessions;
mod password;
mod pins;
mod sessions;
mod setup;
mod types;

use crate::store::Store;

pub const SESSION_SECONDS: i64 = 2_592_000;
pub(crate) use access::{library_access, personal_library, recording_access, shared_library};
pub use types::{
    AccountDeletion, AccountSummary, Actor, AuthSession, ChangePassword, Credentials,
    DeleteAccount, Library, LockStatus, RemovePin, SessionInfo, SessionsRevoked, SetPin, Unlock,
    UpdateDisplayName, User,
};

#[derive(Clone)]
pub struct Identity {
    pub(crate) store: Store,
    pub(crate) password_attempts: password::PasswordAttempts,
    pub(crate) seen_attempts: sessions::SeenAttempts,
}
