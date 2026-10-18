use crate::{InitError, error::Error};
use axum::http::HeaderMap;
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;
use zeroize::Zeroizing;

pub(crate) struct Authentication {
    digest: [u8; 32],
}

impl Authentication {
    pub(crate) fn new(token: String) -> Result<Self, InitError> {
        let token = Zeroizing::new(token);
        if !valid_token(&token) {
            return Err(InitError::InvalidServiceCredential);
        }
        Ok(Self {
            digest: Sha256::digest(token.as_bytes()).into(),
        })
    }

    pub(crate) fn check(&self, headers: &HeaderMap) -> Result<(), Error> {
        let mut values = headers.get_all("authorization").iter();
        let token = values
            .next()
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.strip_prefix("Bearer "))
            .filter(|token| valid_token(token))
            .ok_or(Error::Unauthorized)?;
        if values.next().is_some() {
            return Err(Error::Unauthorized);
        }
        let supplied: [u8; 32] = Sha256::digest(token.as_bytes()).into();
        if !bool::from(supplied.ct_eq(&self.digest)) {
            return Err(Error::Unauthorized);
        }
        let mut protocol = headers.get_all("x-hito-plugin-protocol").iter();
        if protocol.next().is_none_or(|value| value != "1") || protocol.next().is_some() {
            return Err(Error::InvalidRequest);
        }
        Ok(())
    }
}

fn valid_token(token: &str) -> bool {
    !token.is_empty()
        && token.len() <= 4096
        && token.bytes().all(|byte| (0x21..=0x7e).contains(&byte))
}
