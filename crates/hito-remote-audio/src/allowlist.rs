use crate::Error;
use reqwest::Url;

pub struct MediaAllowlist<E = Error> {
    origins: Vec<String>,
    error: std::marker::PhantomData<fn() -> E>,
}

impl<E: From<Error>> MediaAllowlist<E> {
    pub fn new(origins: Vec<String>) -> Result<Self, E> {
        let origins = accepted(origins).map_err(E::from)?;
        Ok(Self {
            origins,
            error: std::marker::PhantomData,
        })
    }
}

impl<E> MediaAllowlist<E> {
    pub(super) fn target(&self, value: &str) -> Result<Url, Error> {
        let target = parse(value)?;
        if !self
            .origins
            .contains(&target.origin().ascii_serialization())
        {
            return Err(Error::MediaDestination);
        }
        Ok(target)
    }
}

fn accepted(origins: Vec<String>) -> Result<Vec<String>, Error> {
    if origins.is_empty() || origins.len() > 32 {
        return Err(Error::Configuration);
    }
    let mut accepted = Vec::with_capacity(origins.len());
    for origin in origins {
        let target = parse(&origin).map_err(|_| Error::Configuration)?;
        if target.path() != "/" || target.query().is_some() {
            return Err(Error::Configuration);
        }
        accepted.push(target.origin().ascii_serialization());
    }
    accepted.sort();
    accepted.dedup();
    Ok(accepted)
}

fn parse(value: &str) -> Result<Url, Error> {
    if value.len() > 8192
        || value.is_empty()
        || value.chars().any(|character| {
            character.is_whitespace() || character.is_control() || character == '\\'
        })
    {
        return Err(Error::MediaDestination);
    }
    let target = Url::parse(value).map_err(|_| Error::MediaDestination)?;
    let host = target.host_str().ok_or(Error::MediaDestination)?;
    if target.scheme() != "https"
        || target.port_or_known_default() != Some(443)
        || !target.username().is_empty()
        || target.password().is_some()
        || target.fragment().is_some()
        || host.len() > 253
        || !host.contains('.')
        || host.ends_with('.')
        || host.parse::<std::net::IpAddr>().is_ok()
        || host.split('.').any(|label| {
            label.is_empty()
                || label.len() > 63
                || label.starts_with('-')
                || label.ends_with('-')
                || !label
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        })
        || value.split_once("://").is_none_or(|(_, rest)| {
            rest.split(['/', '?', '#'])
                .next()
                .is_some_and(|authority| authority.contains('@'))
        })
    {
        return Err(Error::MediaDestination);
    }
    Ok(target)
}
