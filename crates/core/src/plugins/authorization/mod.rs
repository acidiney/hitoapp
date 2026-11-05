mod access;
mod callback;
mod connection;
mod exchange;
mod launch;
mod types;

use super::Plugins;
use crate::{Actor, Error, Result};
pub(crate) use access::ConnectionLocks;
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
pub(super) use connection::retain_declared;
pub(crate) use exchange::TokenClient;
use hito_plugin_sdk::{Authorization, Manifest};
use rand::{RngCore, rngs::OsRng};
use serde::Serialize;
use sha2::{Digest, Sha256};
use sqlx::Row;
use std::time::Instant;
pub use types::{
    PluginAuthorization, PluginAuthorizationCallback, PluginAuthorizationOutcome,
    PluginAuthorizationRedirect, PluginAuthorizationState, PluginAuthorizationStatus,
    PluginConnectionState, PluginConnectionStatus, ProviderAccess,
};
use url::Url;
use uuid::Uuid;
use zeroize::Zeroizing;

impl Plugins {
    #[tracing::instrument(level = "info", skip_all, err(Debug), fields(account_id = actor.id(), installation_id = id))]
    pub async fn begin_authorization(
        &self,
        actor: &Actor,
        id: &str,
        expected_revision: i64,
        redirect: &PluginAuthorizationRedirect,
    ) -> Result<PluginAuthorization> {
        let started = Instant::now();
        let row = sqlx::query("SELECT p.revision,p.service_id,p.manifest,s.revision AS service_revision
            FROM plugin_installations p JOIN plugin_services s ON s.id = p.service_id AND s.active = 1
            WHERE p.id = ? AND p.user_id = ? AND p.active = 1")
            .bind(id).bind(actor.id()).fetch_optional(&self.store.pool).await?.ok_or(Error::NotFound)?;
        let revision: i64 = row.try_get("revision")?;
        if revision != expected_revision {
            return Err(changed());
        }
        let manifest: Manifest = serde_json::from_str(row.try_get("manifest")?)
            .map_err(|_| Error::Internal("Stored plugin manifest is invalid".into()))?;
        manifest
            .validate()
            .map_err(|_| Error::Internal("Stored plugin manifest is invalid".into()))?;
        let Authorization::Oauth2Pkce {
            client_id,
            authorization_endpoint,
            scopes,
            ..
        } = manifest.authorization.ok_or_else(|| {
            Error::Invalid("This plugin does not offer account authorization".into())
        })?;
        let service: String = row.try_get("service_id")?;
        let service_revision: i64 = row.try_get("service_revision")?;
        let attempt_id = Uuid::new_v4().to_string();
        let state = random_secret()?;
        let verifier = random_secret()?;
        let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
        let mut authorization_url = Url::parse(&authorization_endpoint).map_err(|_| {
            Error::Internal("Stored plugin authorization endpoint is invalid".into())
        })?;
        authorization_url
            .query_pairs_mut()
            .append_pair("response_type", "code")
            .append_pair("client_id", &client_id)
            .append_pair("redirect_uri", redirect.as_str())
            .append_pair("scope", &scopes.into_iter().collect::<Vec<_>>().join(" "))
            .append_pair("state", &state)
            .append_pair("code_challenge_method", "S256")
            .append_pair("code_challenge", &challenge);
        let plaintext = Zeroizing::new(
            serde_json::to_vec(&PendingSecrets {
                state: &state,
                verifier: &verifier,
            })
            .map_err(|_| {
                Error::Internal("Plugin authorization secrets could not be encoded".into())
            })?,
        );
        let secret = self.vault.seal_pending_authorization(
            actor.id(),
            id,
            &service,
            &attempt_id,
            &plaintext,
        )?;
        let state_hash = Sha256::digest(state.as_bytes());
        let mut tx = self.store.pool.begin_with("BEGIN IMMEDIATE").await?;
        actor.check_session_on(&mut tx).await?;
        let current: Option<(i64, i64)> = sqlx::query_as("SELECT p.revision,s.revision
            FROM plugin_installations p JOIN plugin_services s ON s.id = p.service_id AND s.active = 1
            WHERE p.id = ? AND p.user_id = ? AND p.service_id = ? AND p.active = 1")
            .bind(id).bind(actor.id()).bind(&service).fetch_optional(&mut *tx).await?;
        if current.ok_or(Error::NotFound)? != (revision, service_revision) {
            return Err(changed());
        }
        let removed = sqlx::query("DELETE FROM plugin_authorization_attempts WHERE id IN (
            SELECT id FROM plugin_authorization_attempts WHERE expires_at <= unixepoch() ORDER BY expires_at,id LIMIT 64)")
            .execute(&mut *tx).await?.rows_affected();
        let pending: i64 = sqlx::query_scalar("SELECT count(*) FROM plugin_authorization_attempts WHERE user_id = ? AND expires_at > unixepoch() AND status IN ('pending','exchanging')")
            .bind(actor.id()).fetch_one(&mut *tx).await?;
        if pending >= 8 {
            return Err(Error::Conflict("Your account has eight pending provider connections. Finish one or wait ten minutes before trying again.".into()));
        }
        let expires_at: i64 = sqlx::query_scalar("INSERT INTO plugin_authorization_attempts
            (id,user_id,session_hash,installation_id,service_id,installation_revision,service_revision,redirect_uri,state_hash,pending_secret,expires_at)
            VALUES (?,?,?,?,?,?,?,?,?,?,unixepoch() + 600) RETURNING expires_at")
            .bind(&attempt_id).bind(actor.id()).bind(actor.session_binding()).bind(id).bind(&service)
            .bind(revision).bind(service_revision).bind(redirect.as_str()).bind(&state_hash[..]).bind(secret)
            .fetch_one(&mut *tx).await?;
        tx.commit().await?;
        tracing::info!(
            attempt_id,
            expired_removed = removed,
            elapsed_ms = started.elapsed().as_millis() as u64,
            outcome = "pending",
            "Provider authorization started"
        );
        Ok(PluginAuthorization {
            id: attempt_id,
            authorization_url: authorization_url.into(),
            expires_at,
        })
    }

    #[tracing::instrument(level = "debug", skip_all, err(Debug), fields(account_id = actor.id(), installation_id = id))]
    pub async fn authorization_status(
        &self,
        actor: &Actor,
        id: &str,
        attempt_id: &str,
    ) -> Result<PluginAuthorizationStatus> {
        let started = Instant::now();
        actor.check_session(&self.store).await?;
        let (id, expires_at, state): (String, i64, String) = sqlx::query_as("SELECT a.id,a.expires_at,a.status FROM plugin_authorization_attempts a
            JOIN sessions session ON session.token_hash = a.session_hash AND session.user_id = a.user_id AND session.expires_at > unixepoch()
            LEFT JOIN plugin_installations p ON p.id = a.installation_id AND p.user_id = a.user_id AND p.service_id = a.service_id AND p.active = 1 AND p.revision = a.installation_revision
            LEFT JOIN plugin_services s ON s.id = a.service_id AND s.active = 1 AND s.revision = a.service_revision
            WHERE a.id = ? AND a.installation_id = ? AND a.user_id = ? AND a.session_hash = ? AND a.expires_at > unixepoch()
              AND (a.status IN ('completed','failed') OR (p.id IS NOT NULL AND s.id IS NOT NULL))")
            .bind(attempt_id).bind(id).bind(actor.id()).bind(actor.session_binding())
            .fetch_optional(&self.store.pool).await?.ok_or(Error::NotFound)?;
        let status = match state.as_str() {
            "pending" => PluginAuthorizationState::Pending,
            "exchanging" => PluginAuthorizationState::Exchanging,
            "completed" => PluginAuthorizationState::Completed,
            _ => PluginAuthorizationState::Failed,
        };
        tracing::debug!(
            attempt_id = id,
            elapsed_ms = started.elapsed().as_millis() as u64,
            outcome = state,
            "Provider authorization inspected"
        );
        Ok(PluginAuthorizationStatus {
            id,
            status,
            expires_at,
        })
    }
}

#[derive(Serialize)]
struct PendingSecrets<'a> {
    state: &'a str,
    verifier: &'a str,
}

fn random_secret() -> Result<Zeroizing<String>> {
    let mut bytes = Zeroizing::new([0u8; 32]);
    OsRng.try_fill_bytes(bytes.as_mut()).map_err(|_| {
        Error::Internal("Secure provider authorization could not be initialized".into())
    })?;
    Ok(Zeroizing::new(URL_SAFE_NO_PAD.encode(&bytes[..])))
}

fn changed() -> Error {
    Error::Conflict(
        "This plugin connection changed. Reload Settings before connecting your provider account."
            .into(),
    )
}
