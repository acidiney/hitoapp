use crate::{
    error::{ApiError, ApiResult},
    security,
};
use axum::{extract::FromRequestParts, http::request::Parts};
use hito_core::{Actor, Error, Services};

pub struct Account(pub Actor);
pub struct OptionalAccount(pub Option<Actor>);
pub struct LockableAccount(pub Actor);

impl FromRequestParts<Services> for Account {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, services: &Services) -> ApiResult<Self> {
        let LockableAccount(actor) = LockableAccount::from_request_parts(parts, services).await?;
        actor.require_unlocked()?;
        Ok(Self(actor))
    }
}

impl FromRequestParts<Services> for LockableAccount {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, services: &Services) -> ApiResult<Self> {
        let OptionalAccount(actor) = OptionalAccount::from_request_parts(parts, services).await?;
        Ok(Self(actor.ok_or(Error::Unauthorized)?))
    }
}

impl FromRequestParts<Services> for OptionalAccount {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, services: &Services) -> ApiResult<Self> {
        let actor = match security::token(&parts.headers) {
            Ok(token) => match services.identity.authenticate(token).await {
                Ok(actor) => actor,
                Err(Error::Unauthorized) => {
                    tracing::debug!(event = "session_rejected", "Session is invalid or expired");
                    return Ok(Self(None));
                }
                Err(error) => return Err(error.into()),
            },
            Err(_) => return Ok(Self(None)),
        };
        if parts
            .headers
            .get("x-hito-account")
            .is_some_and(|account| account != actor.user().id.as_str())
        {
            tracing::warn!(
                event = "account_mismatch",
                "Session account does not match the requested account"
            );
            return Err(Error::Forbidden.into());
        }
        Ok(Self(Some(actor)))
    }
}
