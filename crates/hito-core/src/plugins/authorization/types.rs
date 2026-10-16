use crate::{Error, Result};
use serde::Serialize;
use url::{Host, Url};
use utoipa::ToSchema;

#[derive(Serialize, ToSchema)]
pub struct PluginAuthorization {
    pub id: String,
    pub authorization_url: String,
    pub expires_at: i64,
}

#[derive(Serialize, ToSchema)]
pub struct PluginAuthorizationStatus {
    pub id: String,
    pub status: PluginAuthorizationState,
    pub expires_at: i64,
}

#[derive(Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum PluginAuthorizationState {
    Pending,
}

pub struct PluginAuthorizationRedirect(Url);

impl PluginAuthorizationRedirect {
    pub fn new(callback: &str) -> Result<Self> {
        let invalid = || {
            Error::Invalid("Configure HITO_PUBLIC_ORIGIN as an HTTPS origin or a loopback IP over HTTP before connecting this provider".into())
        };
        if callback.len() > 2048
            || callback
                .chars()
                .any(|value| value.is_control() || value.is_whitespace())
        {
            return Err(invalid());
        }
        let callback = Url::parse(callback).map_err(|_| invalid())?;
        let allowed_scheme = callback.scheme() == "https"
            || (callback.scheme() == "http"
                && match callback.host() {
                    Some(Host::Ipv4(address)) => address.is_loopback(),
                    Some(Host::Ipv6(address)) => address.is_loopback(),
                    _ => false,
                });
        if !allowed_scheme
            || callback.host().is_none()
            || !callback.username().is_empty()
            || callback.password().is_some()
            || callback.query().is_some()
            || callback.fragment().is_some()
            || callback.path() != "/api/v1/plugin-authorization/callback"
        {
            return Err(invalid());
        }
        Ok(Self(callback))
    }

    pub(super) fn as_str(&self) -> &str {
        self.0.as_str()
    }
}
