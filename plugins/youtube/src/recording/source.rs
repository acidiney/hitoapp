use super::identity::Identity;
use crate::Error;
use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use url::Url;

#[derive(Clone)]
pub(crate) struct Descriptor {
    pub(crate) id: String,
    pub(crate) length: u64,
    pub(crate) expires: Instant,
    url: Url,
    validator: Arc<Mutex<Option<String>>>,
}

impl Descriptor {
    pub(super) fn new(owner: &str, format: &str, length: u64, value: &str) -> Result<Self, Error> {
        if value.len() > 8192
            || !value.is_ascii()
            || value.bytes().any(|byte| byte.is_ascii_control())
        {
            return Err(Error::InvalidResponse);
        }
        let url = Url::parse(value).map_err(|_| Error::InvalidResponse)?;
        let host = url.host_str().ok_or(Error::InvalidResponse)?;
        let prefix = host
            .strip_suffix(".googlevideo.com")
            .ok_or(Error::InvalidResponse)?;
        if url.scheme() != "https"
            || url.port_or_known_default() != Some(443)
            || !url.username().is_empty()
            || url.password().is_some()
            || url.fragment().is_some()
            || url.path() != "/videoplayback"
            || prefix.is_empty()
            || prefix.split('.').any(|label| {
                label.is_empty()
                    || label.len() > 63
                    || !label.as_bytes()[0].is_ascii_alphanumeric()
                    || !label.as_bytes()[label.len() - 1].is_ascii_alphanumeric()
                    || !label
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
            })
        {
            return Err(Error::InvalidResponse);
        }
        let mut expiry = None;
        let mut modified = None;
        for (key, value) in url.query_pairs() {
            match key.as_ref() {
                "expire" if expiry.is_none() => expiry = Some(decimal(&value)?),
                "lmt" if modified.is_none() => modified = Some(decimal(&value)?),
                "expire" | "lmt" => return Err(Error::InvalidResponse),
                _ => {}
            }
        }
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| Error::Unavailable)?
            .as_secs();
        let remaining = expiry
            .ok_or(Error::InvalidResponse)?
            .checked_sub(now)
            .and_then(|seconds| seconds.checked_sub(60))
            .filter(|seconds| *seconds > 0)
            .ok_or(Error::InvalidResponse)?;
        let identity = Identity::new(
            owner,
            format,
            length,
            modified.ok_or(Error::InvalidResponse)?,
        )?;
        Ok(Self {
            id: identity.encode(),
            length,
            expires: Instant::now() + Duration::from_secs(remaining.min(300)),
            url,
            validator: Arc::new(Mutex::new(None)),
        })
    }

    pub(crate) fn endpoint(&self) -> Result<&Url, Error> {
        if Instant::now() >= self.expires {
            return Err(Error::Unavailable);
        }
        Ok(&self.url)
    }

    pub(crate) fn verify_validator(&self, received: Option<&str>) -> Result<(), Error> {
        if received.is_some_and(|value| {
            value.len() > 128
                || value.starts_with("W/")
                || !value.starts_with('"')
                || !value.ends_with('"')
                || value.chars().any(char::is_control)
        }) {
            return Err(Error::InvalidMedia);
        }
        let mut pinned = self
            .validator
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        match (&*pinned, received) {
            (Some(before), Some(after)) if before == after => Ok(()),
            (Some(_), _) => Err(Error::ValidatorChanged),
            (None, Some(after)) => {
                *pinned = Some(after.to_owned());
                Ok(())
            }
            (None, None) => Ok(()),
        }
    }
}

fn decimal(value: &str) -> Result<u64, Error> {
    if value.is_empty() || value.len() > 20 || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(Error::InvalidResponse);
    }
    value.parse().map_err(|_| Error::InvalidResponse)
}
