use super::connection::{NewConnection, declaration_digest, scope_list, store};
use super::exchange::Redemption;
use super::{PluginAuthorizationCallback, PluginAuthorizationOutcome};
use crate::plugins::Plugins;
use crate::{Actor, Error, Result};
use hito_plugin_sdk::{Authorization, Manifest};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use sqlx::Row;
use std::time::Instant;
use tracing::{Instrument, instrument::WithSubscriber};
use uuid::Uuid;
use zeroize::Zeroizing;

#[derive(Deserialize)]
struct PendingSecrets {
    state: Zeroizing<String>,
    verifier: Zeroizing<String>,
}

struct Consumed {
    attempt_id: String,
    account_id: String,
    installation_id: String,
    service_id: String,
    installation_revision: i64,
    service_revision: i64,
    redirect_uri: String,
    client_id: String,
    token_endpoint: String,
    code: Zeroizing<String>,
    verifier: Zeroizing<String>,
    digest: [u8; 32],
    requested_scopes: String,
}

enum Claim {
    Done(PluginAuthorizationOutcome),
    Exchange(Box<Consumed>),
}

impl Plugins {
    #[tracing::instrument(level = "info", skip_all, err(Debug), fields(account_id = actor.id(), callback_id = %Uuid::new_v4()))]
    pub async fn complete_authorization(
        &self,
        actor: &Actor,
        callback: PluginAuthorizationCallback,
    ) -> Result<PluginAuthorizationOutcome> {
        let started = Instant::now();
        let admission = if callback.code.is_some() && callback.error.is_none() {
            Some(self.token.admit(actor.id())?)
        } else {
            None
        };
        let outcome = match self.claim(actor, &callback).await? {
            Claim::Done(outcome) => {
                tracing::info!(
                    outcome = outcome.as_str(),
                    elapsed_ms = started.elapsed().as_millis() as u64,
                    "Provider authorization callback rejected or closed"
                );
                outcome
            }
            Claim::Exchange(consumed) => {
                let plugins = self.clone();
                let admission = admission.ok_or_else(|| {
                    Error::Internal("Provider authorization exchange was not admitted".into())
                })?;
                tokio::spawn(
                    async move {
                        let _admission = admission;
                        plugins.exchange_and_finish(*consumed, started).await
                    }
                    .in_current_span()
                    .with_current_subscriber(),
                )
                .await
                .map_err(|_| Error::Internal("Provider authorization exchange stopped".into()))??
            }
        };
        Ok(outcome)
    }

    async fn claim(&self, actor: &Actor, callback: &PluginAuthorizationCallback) -> Result<Claim> {
        let state_hash = Sha256::digest(callback.state.as_bytes());
        let mut tx = self.store.pool.begin_with("BEGIN IMMEDIATE").await?;
        actor.check_session_on(&mut tx).await?;
        let Some(row) = sqlx::query("SELECT a.id,a.installation_id,a.service_id,a.installation_revision,a.service_revision,a.redirect_uri,a.pending_secret,a.status,
                a.expires_at > unixepoch() AS live, (p.id IS NOT NULL AND s.id IS NOT NULL) AS current, p.manifest
            FROM plugin_authorization_attempts a
            LEFT JOIN plugin_installations p ON p.id = a.installation_id AND p.user_id = a.user_id AND p.service_id = a.service_id AND p.active = 1 AND p.revision = a.installation_revision
            LEFT JOIN plugin_services s ON s.id = a.service_id AND s.active = 1 AND s.revision = a.service_revision
            WHERE a.state_hash = ? AND a.user_id = ? AND a.session_hash = ?")
            .bind(&state_hash[..]).bind(actor.id()).bind(actor.session_binding())
            .fetch_optional(&mut *tx).await? else {
            return Ok(Claim::Done(PluginAuthorizationOutcome::Invalid));
        };
        let attempt_id: String = row.try_get("id")?;
        let status: String = row.try_get("status")?;
        if status != "pending" {
            return Ok(Claim::Done(PluginAuthorizationOutcome::Used));
        }
        if !row.try_get::<bool, _>("live")? {
            return Ok(Claim::Done(PluginAuthorizationOutcome::Expired));
        }
        let installation_id: String = row.try_get("installation_id")?;
        let service_id: String = row.try_get("service_id")?;
        let envelope: Vec<u8> = row.try_get("pending_secret")?;
        let secrets = self
            .vault
            .reveal_pending_authorization(
                actor.id(),
                &installation_id,
                &service_id,
                &attempt_id,
                &envelope,
            )
            .ok()
            .and_then(|plain| serde_json::from_str::<PendingSecrets>(&plain).ok());
        let Some(secrets) = secrets else {
            close(&mut tx, &attempt_id, "pending", "failed").await?;
            tx.commit().await?;
            tracing::warn!(
                attempt_id,
                reason = "secrets_unavailable",
                "Provider authorization attempt closed"
            );
            return Ok(Claim::Done(PluginAuthorizationOutcome::ExchangeFailed));
        };
        if !same_secret(&secrets.state, &callback.state) {
            return Ok(Claim::Done(PluginAuthorizationOutcome::Invalid));
        }
        if !row.try_get::<bool, _>("current")? {
            close(&mut tx, &attempt_id, "pending", "failed").await?;
            tx.commit().await?;
            return Ok(Claim::Done(PluginAuthorizationOutcome::Changed));
        }
        if let Some(error) = &callback.error {
            close(&mut tx, &attempt_id, "pending", "failed").await?;
            tx.commit().await?;
            return Ok(Claim::Done(if error == "access_denied" {
                PluginAuthorizationOutcome::Denied
            } else {
                PluginAuthorizationOutcome::ProviderError
            }));
        }
        let Some(code) = &callback.code else {
            return Ok(Claim::Done(PluginAuthorizationOutcome::Invalid));
        };
        let manifest: Option<Manifest> = row
            .try_get::<Option<String>, _>("manifest")?
            .and_then(|text| serde_json::from_str(&text).ok())
            .filter(|manifest: &Manifest| manifest.validate().is_ok());
        let Some(authorization) = manifest.and_then(|manifest| manifest.authorization) else {
            close(&mut tx, &attempt_id, "pending", "failed").await?;
            tx.commit().await?;
            return Ok(Claim::Done(PluginAuthorizationOutcome::Changed));
        };
        let Authorization::Oauth2Pkce {
            client_id,
            token_endpoint,
            scopes,
            ..
        } = &authorization;
        let digest = declaration_digest(&authorization)?;
        close(&mut tx, &attempt_id, "pending", "exchanging").await?;
        tx.commit().await?;
        Ok(Claim::Exchange(Box::new(Consumed {
            account_id: actor.id().to_owned(),
            installation_revision: row.try_get("installation_revision")?,
            service_revision: row.try_get("service_revision")?,
            redirect_uri: row.try_get("redirect_uri")?,
            attempt_id,
            installation_id,
            service_id,
            client_id: client_id.clone(),
            token_endpoint: token_endpoint.clone(),
            code: code.clone(),
            verifier: secrets.verifier.clone(),
            digest,
            requested_scopes: scopes.iter().cloned().collect::<Vec<_>>().join(" "),
        })))
    }

    async fn exchange_and_finish(
        &self,
        consumed: Consumed,
        started: Instant,
    ) -> Result<PluginAuthorizationOutcome> {
        let exchanged = self
            .token
            .redeem(Redemption {
                endpoint: &consumed.token_endpoint,
                client_id: &consumed.client_id,
                redirect_uri: &consumed.redirect_uri,
                code: &consumed.code,
                verifier: &consumed.verifier,
            })
            .await;
        let sealed = match &exchanged {
            Ok(grant) => Some(self.seal_tokens(
                &consumed.account_id,
                &consumed.installation_id,
                &consumed.service_id,
                &consumed.digest,
                &grant.access_token,
                grant.refresh_token.as_ref().map(|token| token.as_str()),
            )),
            Err(_) => None,
        };
        let mut tx = self.store.pool.begin_with("BEGIN IMMEDIATE").await?;
        let outcome = match (&exchanged, sealed) {
            (Ok(grant), Some(Ok(sealed))) => {
                let completed = sqlx::query("UPDATE plugin_authorization_attempts
                    SET status = 'completed', pending_secret = zeroblob(74)
                    WHERE id = ?1 AND status = 'exchanging' AND user_id = ?2 AND expires_at > unixepoch()
                      AND EXISTS(SELECT 1 FROM sessions WHERE token_hash = session_hash AND expires_at > unixepoch())
                      AND EXISTS(SELECT 1 FROM plugin_installations WHERE id = ?3 AND user_id = ?2 AND service_id = ?4 AND active = 1 AND revision = ?5)
                      AND EXISTS(SELECT 1 FROM plugin_services WHERE id = ?4 AND active = 1 AND revision = ?6)")
                    .bind(&consumed.attempt_id).bind(&consumed.account_id).bind(&consumed.installation_id)
                    .bind(&consumed.service_id).bind(consumed.installation_revision).bind(consumed.service_revision)
                    .execute(&mut *tx).await?.rows_affected();
                if completed == 1 {
                    let scopes = scope_list(grant.scope.as_deref())
                        .unwrap_or_else(|| consumed.requested_scopes.clone());
                    store(
                        &mut tx,
                        &NewConnection {
                            account: &consumed.account_id,
                            installation: &consumed.installation_id,
                            service: &consumed.service_id,
                            digest: &consumed.digest,
                            sealed: &sealed,
                            scopes: &scopes,
                            expires_in: grant.expires_in,
                        },
                    )
                    .await?;
                    PluginAuthorizationOutcome::Connected
                } else {
                    close(&mut tx, &consumed.attempt_id, "exchanging", "failed").await?;
                    PluginAuthorizationOutcome::Changed
                }
            }
            _ => {
                close(&mut tx, &consumed.attempt_id, "exchanging", "failed").await?;
                PluginAuthorizationOutcome::ExchangeFailed
            }
        };
        tx.commit().await?;
        match exchanged {
            Ok(grant) => tracing::info!(
                attempt_id = consumed.attempt_id,
                installation_id = consumed.installation_id,
                outcome = outcome.as_str(),
                refresh_token_offered = grant.refresh_token.is_some(),
                expires_in_seconds = grant.expires_in,
                elapsed_ms = started.elapsed().as_millis() as u64,
                "Provider authorization code exchanged"
            ),
            Err(failure) => tracing::warn!(
                attempt_id = consumed.attempt_id,
                outcome = outcome.as_str(),
                reason = failure.as_str(),
                elapsed_ms = started.elapsed().as_millis() as u64,
                "Provider authorization code exchange failed"
            ),
        }
        Ok(outcome)
    }
}

async fn close(
    connection: &mut sqlx::SqliteConnection,
    attempt_id: &str,
    from: &str,
    to: &str,
) -> Result<()> {
    sqlx::query("UPDATE plugin_authorization_attempts SET status = ?, pending_secret = zeroblob(74) WHERE id = ? AND status = ?")
        .bind(to)
        .bind(attempt_id)
        .bind(from)
        .execute(connection)
        .await?;
    Ok(())
}

fn same_secret(left: &str, right: &str) -> bool {
    let left = Sha256::digest(left.as_bytes());
    let right = Sha256::digest(right.as_bytes());
    left.iter()
        .zip(right.iter())
        .fold(0u8, |difference, (a, b)| difference | (a ^ b))
        == 0
}
