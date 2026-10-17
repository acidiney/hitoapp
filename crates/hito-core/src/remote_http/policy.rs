use crate::{Error, Result};
use std::{cell::Cell, net::IpAddr};
use url::{SyntaxViolation, Url};

#[derive(Clone, Default)]
pub struct PodcastNetworkConfig {
    trusted_origins: Vec<String>,
}

impl PodcastNetworkConfig {
    pub fn new(trusted_origins: Vec<String>) -> Result<Self> {
        if trusted_origins.len() > 32 {
            return Err(Error::Invalid("Too many trusted podcast origins".into()));
        }
        let mut origins = Vec::new();
        for value in trusted_origins {
            let origin = parse(&value)?;
            if origin.query().is_some() || origin.path() != "/" {
                return Err(Error::Invalid("A trusted podcast origin is invalid".into()));
            }
            origins.push(origin.origin().ascii_serialization());
        }
        origins.sort();
        origins.dedup();
        Ok(Self {
            trusted_origins: origins,
        })
    }

    pub(super) fn permits(&self, target: &Url, addresses: &[IpAddr]) -> bool {
        !addresses.is_empty()
            && addresses.len() <= 32
            && (self
                .trusted_origins
                .contains(&target.origin().ascii_serialization())
                || addresses.iter().all(public_address))
    }
}

pub(crate) fn parse(value: &str) -> Result<Url> {
    parse_reference(value, None)
}

pub(super) fn redirect(current: &Url, location: &str) -> Result<Url> {
    parse_reference(location, Some(current))
}

fn parse_reference(value: &str, base: Option<&Url>) -> Result<Url> {
    if value.len() > 2048 || value.trim() != value || value.chars().any(char::is_control) {
        return Err(invalid());
    }
    let credentials = Cell::new(false);
    let check_syntax = |violation| {
        if matches!(violation, SyntaxViolation::EmbeddedCredentials) {
            credentials.set(true);
        }
    };
    let target = Url::options()
        .base_url(base)
        .syntax_violation_callback(Some(&check_syntax))
        .parse(value)
        .map_err(|_| invalid())?;
    if credentials.get() {
        return Err(invalid());
    }
    validate(&target)?;
    Ok(target)
}

fn validate(target: &Url) -> Result<()> {
    if target.as_str().len() > 2048
        || !matches!(target.scheme(), "http" | "https")
        || target.host().is_none()
        || !target.username().is_empty()
        || target.password().is_some()
        || target.fragment().is_some()
        || target.port_or_known_default().is_none_or(|port| port == 0)
    {
        return Err(invalid());
    }
    Ok(())
}

fn invalid() -> Error {
    Error::Invalid("The remote podcast address is invalid".into())
}

fn public_address(address: &IpAddr) -> bool {
    match address {
        IpAddr::V4(address) => {
            let [a, b, c, _] = address.octets();
            !(a == 0
                || a == 10
                || a == 127
                || a >= 224
                || (a == 100 && (64..=127).contains(&b))
                || (a == 169 && b == 254)
                || (a == 172 && (16..=31).contains(&b))
                || (a == 192 && b == 168)
                || (a == 192 && b == 0 && (c == 0 || c == 2))
                || (a == 192 && b == 88 && c == 99)
                || (a == 198 && (b == 18 || b == 19))
                || (a == 198 && b == 51 && c == 100)
                || (a == 203 && b == 0 && c == 113))
        }
        IpAddr::V6(address) => {
            let segments = address.segments();
            let octets = address.octets();
            segments[0] & 0xe000 == 0x2000
                && !(segments[0] == 0x2001 && (segments[1] < 0x0200 || segments[1] == 0x0db8))
                && segments[0] != 0x2002
                && segments[0] != 0x3ffe
                && !(segments[0] == 0x3fff && segments[1] & 0xf000 == 0)
                && !matches!(&octets[8..12], [0, 0, 0x5e, 0xfe] | [2, 0, 0x5e, 0xfe])
        }
    }
}
