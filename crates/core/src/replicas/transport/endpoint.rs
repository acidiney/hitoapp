use crate::{Error, Result};
use url::Url;

#[derive(Clone)]
pub(crate) struct Endpoint(Url);

impl Endpoint {
    pub(crate) fn parse(value: &str) -> Result<Self> {
        if value.trim() != value
            || value.chars().any(char::is_control)
            || value.split_once("://").is_none_or(|(_, rest)| {
                rest.split(['/', '?', '#'])
                    .next()
                    .is_none_or(|authority| authority.contains(['@', '\\']))
            })
        {
            return Err(invalid());
        }
        let url = Url::parse(value).map_err(|_| invalid())?;
        if !matches!(url.scheme(), "http" | "https")
            || url.host_str().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
            || url.path() != "/"
        {
            return Err(invalid());
        }
        Ok(Self(url))
    }

    pub(super) fn route(&self, path: &'static str) -> super::RemoteResult<Url> {
        self.0
            .join(path)
            .map_err(|_| super::RemoteError::InvalidReply)
    }

    pub(crate) fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

fn invalid() -> Error {
    Error::Invalid("Use the root HTTP or HTTPS address of a Hito server".into())
}
