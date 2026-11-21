use crate::{
    auth::Account,
    error::{ApiError, ApiResult},
};
use axum::{extract::FromRequestParts, http::request::Parts};
use hito_core::{Error, Services};
use hito_playback::seam::Listener;
use std::{future::Future, pin::Pin, sync::Arc};

pub type Identified<'a> = Pin<Box<dyn Future<Output = ApiResult<Listener>> + Send + 'a>>;

pub trait Identify: Send + Sync {
    fn identify<'a>(&'a self, parts: &'a mut Parts) -> Identified<'a>;
}

#[derive(Clone)]
pub struct PlayerIdentity(pub Arc<dyn Identify>);

pub struct PlayerListener(pub Listener);

impl<S: Send + Sync> FromRequestParts<S> for PlayerListener {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, _: &S) -> ApiResult<Self> {
        let identity = parts
            .extensions
            .get::<PlayerIdentity>()
            .cloned()
            .ok_or(Error::Unauthorized)?;
        Ok(Self(identity.0.identify(parts).await?))
    }
}

pub struct AccountIdentity(pub Services);

impl Identify for AccountIdentity {
    fn identify<'a>(&'a self, parts: &'a mut Parts) -> Identified<'a> {
        Box::pin(async move {
            let Account(actor) = Account::from_request_parts(parts, &self.0).await?;
            Ok(Listener::from(actor))
        })
    }
}
