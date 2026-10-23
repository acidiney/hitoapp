use super::Credentials;
use crate::{Error, Result};
use argon2::{Argon2, PasswordHash, PasswordHasher, PasswordVerifier, password_hash::SaltString};
use rand::rngs::OsRng;

#[tracing::instrument(level = "debug", skip_all, err(Debug))]
pub(super) fn validate(credentials: &Credentials) -> Result<String> {
    let username = credentials.username.trim();
    if username.is_empty()
        || username.len() > 64
        || !username
            .chars()
            .all(|c| c.is_alphanumeric() || "_- .".contains(c))
    {
        return Err(Error::Invalid(
            "Use 1 to 64 letters, numbers, spaces, underscores, dots, or hyphens for the username"
                .into(),
        ));
    }
    if !(12..=1024).contains(&credentials.password.len()) {
        return Err(Error::Invalid(
            "Use a password between 12 and 1024 bytes".into(),
        ));
    }
    Ok(username.into())
}

#[tracing::instrument(level = "debug", skip_all, err(Debug))]
pub(super) async fn hash(password: String) -> Result<String> {
    tokio::task::spawn_blocking(move || {
        Argon2::default()
            .hash_password(password.as_bytes(), &SaltString::generate(&mut OsRng))
            .map(|hash| hash.to_string())
            .map_err(|e| Error::Internal(e.to_string()))
    })
    .await
    .map_err(|e| Error::Internal(e.to_string()))?
}

#[tracing::instrument(level = "debug", skip_all, err(Debug))]
pub(super) async fn verify(password: String, hash: String) -> Result<bool> {
    tokio::task::spawn_blocking(move || {
        PasswordHash::new(&hash).ok().is_some_and(|hash| {
            Argon2::default()
                .verify_password(password.as_bytes(), &hash)
                .is_ok()
        })
    })
    .await
    .map_err(|e| Error::Internal(e.to_string()))
}
