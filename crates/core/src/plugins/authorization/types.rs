use crate::{Error, Result};
use serde::Serialize;
use url::{Host, Url};
use utoipa::ToSchema;
use zeroize::Zeroizing;

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
    Exchanging,
    Completed,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PluginAuthorizationOutcome {
    Connected,
    Denied,
    ProviderError,
    ExchangeFailed,
    Expired,
    Changed,
    Used,
    Invalid,
}

impl PluginAuthorizationOutcome {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Connected => "connected",
            Self::Denied => "denied",
            Self::ProviderError => "provider_error",
            Self::ExchangeFailed => "exchange_failed",
            Self::Expired => "expired",
            Self::Changed => "changed",
            Self::Used => "used",
            Self::Invalid => "invalid",
        }
    }
}

pub struct PluginAuthorizationCallback {
    pub(super) state: String,
    pub(super) code: Option<Zeroizing<String>>,
    pub(super) error: Option<String>,
}

impl PluginAuthorizationCallback {
    pub fn parse(query: Option<&str>) -> Option<Self> {
        let query = query?;
        if query.len() > 4096 {
            return None;
        }
        let (mut state, mut code, mut error) = (None, None, None);
        for (key, value) in url::form_urlencoded::parse(query.as_bytes()) {
            let slot = match key.as_ref() {
                "state" => &mut state,
                "code" => &mut code,
                "error" => &mut error,
                _ => continue,
            };
            if slot.replace(Zeroizing::new(value.into_owned())).is_some() {
                return None;
            }
        }
        let visible = |value: &str, maximum: usize| {
            !value.is_empty()
                && value.len() <= maximum
                && value.bytes().all(|b| b.is_ascii_graphic())
        };
        let state = state.filter(|value| visible(value, 128))?;
        if code.as_ref().is_some_and(|value| !visible(value, 2048))
            || error.as_ref().is_some_and(|value| !visible(value, 128))
        {
            return None;
        }
        Some(Self {
            state: state.to_string(),
            code,
            error: error.map(|value| value.to_string()),
        })
    }
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
