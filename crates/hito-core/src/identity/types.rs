use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Debug, Clone, Serialize, sqlx::FromRow, ToSchema)]
pub struct User {
    pub id: String,
    pub username: String,
    pub admin: bool,
}

#[derive(Debug, Clone)]
pub struct Actor {
    pub(super) user: User,
}

impl Actor {
    pub fn user(&self) -> &User {
        &self.user
    }

    pub(crate) fn id(&self) -> &str {
        &self.user.id
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
}
