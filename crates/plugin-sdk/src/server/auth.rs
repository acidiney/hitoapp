use axum::http::HeaderMap;
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

pub const PROTOCOL_HEADER: &str = "x-hito-plugin-protocol";
const MAX_TOKEN_BYTES: usize = 4096;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthFailure {
    Unauthorized,
    InvalidProtocol,
}

/// Digest of a service credential, or `None` when the credential is empty,
/// longer than 4096 bytes, or contains anything but visible ASCII.
pub fn credential_digest(value: &str) -> Option<[u8; 32]> {
    if value.is_empty()
        || value.len() > MAX_TOKEN_BYTES
        || !value.bytes().all(|byte| (0x21..=0x7e).contains(&byte))
    {
        return None;
    }
    Some(Sha256::digest(value.as_bytes()).into())
}

/// Digest of the single `Authorization: Bearer` credential in the request.
pub fn bearer_digest(headers: &HeaderMap) -> Result<[u8; 32], AuthFailure> {
    let mut values = headers.get_all("authorization").iter();
    let supplied = values
        .next()
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .and_then(credential_digest)
        .ok_or(AuthFailure::Unauthorized)?;
    if values.next().is_some() {
        return Err(AuthFailure::Unauthorized);
    }
    Ok(supplied)
}

/// Requires exactly one protocol header with the supported version.
pub fn require_protocol(headers: &HeaderMap) -> Result<(), AuthFailure> {
    let mut versions = headers.get_all(PROTOCOL_HEADER).iter();
    if versions.next().is_none_or(|value| value != "1") || versions.next().is_some() {
        return Err(AuthFailure::InvalidProtocol);
    }
    Ok(())
}

#[derive(Clone)]
pub struct ServiceAuth {
    digest: [u8; 32],
}

impl ServiceAuth {
    pub fn new(token: &str) -> Option<Self> {
        credential_digest(token).map(|digest| Self { digest })
    }

    pub fn check(&self, headers: &HeaderMap) -> Result<(), AuthFailure> {
        let supplied = bearer_digest(headers)?;
        if !bool::from(supplied.ct_eq(&self.digest)) {
            return Err(AuthFailure::Unauthorized);
        }
        require_protocol(headers)
    }
}
