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

    pub fn user(&self) -> &User {
        &self.user
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
