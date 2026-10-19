use crate::{Error, Result};
use sha2::{Digest, Sha256};
use std::path::PathBuf;
use url::{Host, Url};
use zeroize::Zeroizing;

pub struct CommunityConfig {
    pub(super) endpoint: Url,
    pub(super) admin_token_file: PathBuf,
    pub(super) service_key: String,
}

pub struct CommunityRankingConfig {
    pub(super) endpoint: Url,
}

impl CommunityRankingConfig {
    pub fn new(endpoint: String) -> Result<Self> {
        let endpoint = origin(&endpoint)
            .ok_or_else(|| Error::Invalid("Use a community ranking HTTPS origin".into()))?;
        Ok(Self { endpoint })
    }
}

impl CommunityConfig {
    pub fn new(endpoint: String, admin_token_file: PathBuf) -> Result<Self> {
        let invalid = || {
            Error::Invalid(
                "Use a community HTTPS origin and an absolute private administrator-token file"
                    .into(),
            )
        };
        if !admin_token_file.is_absolute() || admin_token_file.file_name().is_none() {
            return Err(invalid());
        }
        let endpoint = origin(&endpoint).ok_or_else(invalid)?;
        let service_key = format!("{:x}", Sha256::digest(endpoint.as_str().as_bytes()));
        Ok(Self {
            endpoint,
            admin_token_file,
            service_key,
        })
    }
}

fn origin(value: &str) -> Option<Url> {
    if value.len() > 2048 || value.chars().any(char::is_control) {
        return None;
    }
    let endpoint = Url::parse(value).ok()?;
    let loopback = match endpoint.host() {
        Some(Host::Ipv4(address)) => address.is_loopback(),
        Some(Host::Ipv6(address)) => address.is_loopback(),
        _ => false,
    };
    ((endpoint.scheme() == "https" || (endpoint.scheme() == "http" && loopback))
        && endpoint.host().is_some()
        && endpoint.port_or_known_default() != Some(0)
        && endpoint.username().is_empty()
        && endpoint.password().is_none()
        && endpoint.query().is_none()
        && endpoint.fragment().is_none()
        && endpoint.path() == "/")
        .then_some(endpoint)
}

pub(super) fn valid_token(token: &str) -> bool {
    token.len() == 64
        && token
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

#[cfg(unix)]
pub(super) fn read_admin_token(path: &std::path::Path) -> Result<Zeroizing<String>> {
    use std::{
        fs::OpenOptions,
        io::Read,
        os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
    };
    let unavailable = || Error::Upstream("Community administrator credentials are unavailable");
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
        .map_err(|_| unavailable())?;
    let metadata = file.metadata().map_err(|_| unavailable())?;
    let owner = unsafe { libc::geteuid() };
    if !metadata.is_file()
        || metadata.nlink() != 1
        || metadata.uid() != owner
        || !matches!(metadata.permissions().mode() & 0o7777, 0o400 | 0o600)
        || !(64..=66).contains(&metadata.len())
    {
        return Err(unavailable());
    }
    let mut token = Zeroizing::new(String::new());
    file.take(67)
        .read_to_string(&mut token)
        .map_err(|_| unavailable())?;
    if token.ends_with('\n') {
        token.pop();
        if token.ends_with('\r') {
            token.pop();
        }
    }
    if !valid_token(&token) {
        return Err(unavailable());
    }
    Ok(token)
}

#[cfg(not(unix))]
pub(super) fn read_admin_token(_path: &std::path::Path) -> Result<Zeroizing<String>> {
    Err(Error::Upstream(
        "Community administrator credentials are unavailable on this platform",
    ))
}
