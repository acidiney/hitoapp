use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Debug, Clone, Deserialize, Serialize, sqlx::FromRow, ToSchema)]
pub struct User {
    pub id: String,
    pub username: String,
    pub admin: bool,
    pub display_name: Option<String>,
    pub avatar_version: Option<String>,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow, ToSchema)]
pub struct AccountSummary {
    pub id: String,
    pub username: String,
    pub admin: bool,
    pub display_name: Option<String>,
    pub avatar_version: Option<String>,
    pub last_active_at: Option<i64>,
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

    pub(crate) fn require_foreground_session(&self) -> Result<()> {
        if self.background || self.unattended {
            Err(Error::Forbidden)
        } else {
            Ok(())
        }
    }

    pub(crate) fn require_session(&self) -> Result<()> {
        if self.unattended {
            Err(Error::Forbidden)
        } else {
            Ok(())
        }
    }

    pub(crate) fn require_admin(&self) -> Result<()> {
        if self.user.admin {
            Ok(())
        } else {
            Err(Error::Forbidden)
        }
    }
}

#[derive(Deserialize, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct Credentials {
    pub username: String,
    pub password: String,
}

#[derive(Deserialize, Serialize, ToSchema)]
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
    pub username: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AccountDeletion {
    pub sessions_revoked: u64,
    pub rows_removed: u64,
    pub files_queued: u64,
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateDisplayName {
    #[serde(default)]
    pub display_name: Option<String>,
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ChangePassword {
    pub current_password: String,
    pub new_password: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct SessionInfo {
    pub id: String,
    pub created_at: i64,
    pub last_seen_at: i64,
    pub client: Option<String>,
    pub current: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ToSchema)]
pub struct SessionsRevoked {
    pub revoked: u64,
}

#[cfg(test)]
#[path = "../../tests/playback_leases/foreground.rs"]
mod lease_foreground_tests;
