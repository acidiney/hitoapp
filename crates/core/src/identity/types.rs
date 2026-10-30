use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Debug, Clone, Serialize, sqlx::FromRow, ToSchema)]
pub struct User {
    pub id: String,
    pub username: String,
    pub admin: bool,
}

#[derive(Clone)]
pub struct Actor {
    pub(super) user: User,
    pub(super) session_hash: String,
    pub(super) locked: bool,
    pub(super) background: bool,
    pub(super) unattended: bool,
}

impl std::fmt::Debug for Actor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Actor")
            .field("user", &self.user)
            .finish_non_exhaustive()
    }
}

impl Actor {
    pub fn same_session(&self, other: &Self) -> bool {
        self.user.id == other.user.id && self.session_hash == other.session_hash
    }

    pub fn is_locked(&self) -> bool {
        self.locked
    }

    pub fn require_unlocked(&self) -> Result<()> {
        if self.locked {
            Err(Error::Locked)
        } else {
            Ok(())
        }
    }

    pub fn user(&self) -> &User {
        &self.user
    }

    pub(crate) fn is_unattended(&self) -> bool {
        self.unattended
    }

    pub(crate) fn id(&self) -> &str {
        &self.user.id
    }

    pub(crate) fn session_binding(&self) -> &str {
        &self.session_hash
    }

    pub(crate) fn require_admin(&self) -> Result<()> {
        if self.user.admin {
            Ok(())
        } else {
            Err(Error::Forbidden)
        }
    }
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct Credentials {
    pub username: String,
    pub password: String,
}

#[derive(Serialize, ToSchema)]
pub struct AuthSession {
    pub token: String,
    pub user: User,
}

#[derive(Serialize, sqlx::FromRow, ToSchema)]
pub struct Library {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub personal: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct LockStatus {
    pub pin_enabled: bool,
    pub idle_minutes: u32,
    pub locked: bool,
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct SetPin {
    pub password: String,
    #[serde(default)]
    pub pin: Option<String>,
    pub idle_minutes: u32,
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct RemovePin {
    pub password: String,
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct Unlock {
    pub pin: String,
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct DeleteAccount {
    /// The account name the administrator saw, so a stale or mistaken request cannot erase a different account.
    pub username: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AccountDeletion {
    pub sessions_revoked: u64,
    pub rows_removed: u64,
    pub files_queued: u64,
}
