use super::callback::{Claim, Consumed, PendingSecrets, close, same_digest, same_secret};
use super::connection::declaration_digest;
use super::{
    PluginAppReturn, PluginAuthorizationCallback, PluginAuthorizationCompletion,
    PluginAuthorizationOutcome, PluginAuthorizationReturn, random_secret,
};
use crate::plugins::Plugins;
use crate::{Actor, Error, Result};
use hito_plugin_sdk::{Authorization, Manifest};
use sha2::{Digest, Sha256};
use sqlx::Row;
use std::time::Instant;
use tracing::{Instrument, instrument::WithSubscriber};
use url::Url;
use uuid::Uuid;
use zeroize::Zeroizing;

const APP_RETURN: &str = "hito://plugin-authorization/return";

enum Parked {
    App {
        attempt_id: String,
        secret: Zeroizing<String>,
    },
    Closed(PluginAuthorizationOutcome),
}

impl Plugins {
    #[tracing::instrument(level = "info", skip_all, err(Debug), fields(account_id = tracing::field::Empty, callback_id = %Uuid::new_v4()))]
    pub async fn return_native_authorization(
        &self,
        callback: &PluginAuthorizationCallback,
    ) -> Result<Option<PluginAuthorizationReturn>> {
        let started = Instant::now();
        let state_hash = Sha256::digest(callback.state.as_bytes());
        let account: Option<String> = sqlx::query_scalar(
            "SELECT user_id FROM plugin_authorization_attempts WHERE state_hash = ? AND delivery = 'native'",
        )
        .bind(&state_hash[..])
        .fetch_optional(&self.store.pool)
        .await?;
        let Some(account) = account else {
            return Ok(None);
        };
        tracing::Span::current().record("account_id", account.as_str());
        match self.park(&state_hash, callback).await? {
            Parked::App { attempt_id, secret } => {
                let mut url = Url::parse(APP_RETURN)
                    .map_err(|_| Error::Internal("The app return address is invalid".into()))?;
                url.query_pairs_mut()
                    .append_pair("attempt", &attempt_id)
                    .append_pair("return", &secret);
                tracing::info!(
                    attempt_id,
                    outcome = "returned",
                    elapsed_ms = started.elapsed().as_millis() as u64,
                    "Provider authorization returned to the app"
                );
                Ok(Some(PluginAuthorizationReturn::App(PluginAppReturn(
                    Zeroizing::new(url.into()),
                ))))
            }
            Parked::Closed(outcome) => {
                tracing::info!(
                    outcome = outcome.as_str(),
                    elapsed_ms = started.elapsed().as_millis() as u64,
                    "Provider authorization return rejected or closed"
                );
                Ok(Some(PluginAuthorizationReturn::Closed(outcome)))
            }
        }
    }

    async fn park(
        &self,
        state_hash: &[u8],
        callback: &PluginAuthorizationCallback,
    ) -> Result<Parked> {
        let mut tx = self.store.pool.begin_with("BEGIN IMMEDIATE").await?;
        let Some(row) = sqlx::query("SELECT a.id,a.user_id,a.installation_id,a.service_id,a.pending_secret,a.status,
                a.expires_at > unixepoch() AS live, (p.id IS NOT NULL AND s.id IS NOT NULL) AS current,
                EXISTS(SELECT 1 FROM sessions session WHERE session.token_hash = a.session_hash AND session.user_id = a.user_id AND session.expires_at > unixepoch()) AS signed_in
            FROM plugin_authorization_attempts a
            LEFT JOIN plugin_installations p ON p.id = a.installation_id AND p.user_id = a.user_id AND p.service_id = a.service_id AND p.active = 1 AND p.revision = a.installation_revision
            LEFT JOIN plugin_services s ON s.id = a.service_id AND s.active = 1 AND s.revision = a.service_revision
            WHERE a.state_hash = ? AND a.delivery = 'native'")
            .bind(state_hash).fetch_optional(&mut *tx).await? else {
            return Ok(Parked::Closed(PluginAuthorizationOutcome::Invalid));
        };
        let attempt_id: String = row.try_get("id")?;
        let account: String = row.try_get("user_id")?;
        if row.try_get::<String, _>("status")? != "pending" {
            return Ok(Parked::Closed(PluginAuthorizationOutcome::Used));
        }
        if !row.try_get::<bool, _>("live")? {
            return Ok(Parked::Closed(PluginAuthorizationOutcome::Expired));
        }
        if !row.try_get::<bool, _>("signed_in")? {
            return Ok(Parked::Closed(PluginAuthorizationOutcome::Invalid));
        }
        let installation_id: String = row.try_get("installation_id")?;
        let service_id: String = row.try_get("service_id")?;
        let envelope: Vec<u8> = row.try_get("pending_secret")?;
        let secrets = self
            .vault
            .reveal_pending_authorization(
                &account,
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
            return Ok(Parked::Closed(PluginAuthorizationOutcome::ExchangeFailed));
        };
        if !same_secret(&secrets.state, &callback.state) {
            return Ok(Parked::Closed(PluginAuthorizationOutcome::Invalid));
        }
        if !row.try_get::<bool, _>("current")? {
            close(&mut tx, &attempt_id, "pending", "failed").await?;
            tx.commit().await?;
            return Ok(Parked::Closed(PluginAuthorizationOutcome::Changed));
        }
        if let Some(error) = &callback.error {
            close(&mut tx, &attempt_id, "pending", "failed").await?;
            tx.commit().await?;
            return Ok(Parked::Closed(if error == "access_denied" {
                PluginAuthorizationOutcome::Denied
            } else {
                PluginAuthorizationOutcome::ProviderError
            }));
        }
        let Some(code) = &callback.code else {
            return Ok(Parked::Closed(PluginAuthorizationOutcome::Invalid));
        };
        let Ok(sealed) = self.vault.seal_returned_code(
            &account,
            &installation_id,
            &service_id,
            &attempt_id,
            code,
        ) else {
            close(&mut tx, &attempt_id, "pending", "failed").await?;
            tx.commit().await?;
            return Ok(Parked::Closed(PluginAuthorizationOutcome::ExchangeFailed));
        };
        let secret = random_secret()?;
        let return_hash = Sha256::digest(secret.as_bytes());
        sqlx::query("UPDATE plugin_authorization_attempts SET status = 'returned', returned_code = ?, return_hash = ?
            WHERE id = ? AND status = 'pending'")
            .bind(sealed).bind(&return_hash[..]).bind(&attempt_id)
            .execute(&mut *tx).await?;
        tx.commit().await?;
        Ok(Parked::App { attempt_id, secret })
    }

    #[tracing::instrument(level = "info", skip_all, err(Debug), fields(account_id = actor.id(), installation_id = id, attempt_id = attempt_id))]
    pub async fn complete_native_authorization(
        &self,
        actor: &Actor,
        id: &str,
        attempt_id: &str,
        completion: PluginAuthorizationCompletion,
    ) -> Result<PluginAuthorizationOutcome> {
        let started = Instant::now();
        actor.require_unlocked()?;
        let admission = self.token.admit(actor.id())?;
        match self
            .claim_native(actor, id, attempt_id, &completion)
            .await?
        {
            Claim::Done(outcome) => {
                tracing::info!(
                    outcome = outcome.as_str(),
                    elapsed_ms = started.elapsed().as_millis() as u64,
                    "Provider authorization completion rejected or closed"
                );
                Ok(outcome)
            }
            Claim::Exchange(consumed) => {
                let plugins = self.clone();
                tokio::spawn(
                    async move {
                        let _admission = admission;
                        plugins.exchange_and_finish(*consumed, started).await
                    }
                    .in_current_span()
                    .with_current_subscriber(),
                )
                .await
                .map_err(|_| Error::Internal("Provider authorization exchange stopped".into()))?
            }
        }
    }

    async fn claim_native(
        &self,
        actor: &Actor,
        id: &str,
        attempt_id: &str,
        completion: &PluginAuthorizationCompletion,
    ) -> Result<Claim> {
        let mut tx = self.store.pool.begin_with("BEGIN IMMEDIATE").await?;
        actor.check_session_on(&mut tx).await?;
        let row = sqlx::query("SELECT a.service_id,a.installation_revision,a.service_revision,a.redirect_uri,a.pending_secret,a.status,
                a.completion_hash,a.return_hash,a.returned_code,
                a.expires_at > unixepoch() AS live, (p.id IS NOT NULL AND s.id IS NOT NULL) AS current, p.manifest
            FROM plugin_authorization_attempts a
            LEFT JOIN plugin_installations p ON p.id = a.installation_id AND p.user_id = a.user_id AND p.service_id = a.service_id AND p.active = 1 AND p.revision = a.installation_revision
            LEFT JOIN plugin_services s ON s.id = a.service_id AND s.active = 1 AND s.revision = a.service_revision
            WHERE a.id = ? AND a.installation_id = ? AND a.user_id = ? AND a.session_hash = ? AND a.delivery = 'native'")
            .bind(attempt_id).bind(id).bind(actor.id()).bind(actor.session_binding())
            .fetch_optional(&mut *tx).await?.ok_or(Error::NotFound)?;
        match row.try_get::<String, _>("status")?.as_str() {
            "returned" => {}
            "pending" => return Ok(Claim::Done(PluginAuthorizationOutcome::Invalid)),
            _ => return Ok(Claim::Done(PluginAuthorizationOutcome::Used)),
        }
        if !row.try_get::<bool, _>("live")? {
            close(&mut tx, attempt_id, "returned", "failed").await?;
            tx.commit().await?;
            return Ok(Claim::Done(PluginAuthorizationOutcome::Expired));
        }
        let completion_hash: Vec<u8> = row.try_get("completion_hash")?;
        let return_hash: Vec<u8> = row.try_get("return_hash")?;
        let verified = same_digest(
            &Sha256::digest(completion.verifier.as_bytes()),
            &completion_hash,
        );
        let returned = same_digest(
            &Sha256::digest(completion.return_secret.as_bytes()),
            &return_hash,
        );
        if !(verified & returned) {
            close(&mut tx, attempt_id, "returned", "failed").await?;
            tx.commit().await?;
            return Ok(Claim::Done(PluginAuthorizationOutcome::Invalid));
        }
        let service_id: String = row.try_get("service_id")?;
        let pending: Vec<u8> = row.try_get("pending_secret")?;
        let parked: Vec<u8> = row.try_get("returned_code")?;
        let secrets = self
            .vault
            .reveal_pending_authorization(actor.id(), id, &service_id, attempt_id, &pending)
            .ok()
            .and_then(|plain| serde_json::from_str::<PendingSecrets>(&plain).ok());
        let code = self
            .vault
            .reveal_returned_code(actor.id(), id, &service_id, attempt_id, &parked)
            .ok();
        let (Some(secrets), Some(code)) = (secrets, code) else {
            close(&mut tx, attempt_id, "returned", "failed").await?;
            tx.commit().await?;
            tracing::warn!(
                reason = "secrets_unavailable",
                "Provider authorization attempt closed"
            );
            return Ok(Claim::Done(PluginAuthorizationOutcome::ExchangeFailed));
        };
        let manifest: Option<Manifest> = row
            .try_get::<Option<String>, _>("manifest")?
            .and_then(|text| serde_json::from_str(&text).ok())
            .filter(|manifest: &Manifest| manifest.validate().is_ok());
        let authorization = manifest.and_then(|manifest| manifest.authorization);
        let (true, Some(authorization)) = (row.try_get::<bool, _>("current")?, authorization)
        else {
            close(&mut tx, attempt_id, "returned", "failed").await?;
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
        close(&mut tx, attempt_id, "returned", "exchanging").await?;
        tx.commit().await?;
        Ok(Claim::Exchange(Box::new(Consumed {
            attempt_id: attempt_id.to_owned(),
            account_id: actor.id().to_owned(),
            installation_id: id.to_owned(),
            installation_revision: row.try_get("installation_revision")?,
            service_revision: row.try_get("service_revision")?,
            redirect_uri: row.try_get("redirect_uri")?,
            service_id,
            client_id: client_id.clone(),
            token_endpoint: token_endpoint.clone(),
            code,
            verifier: secrets.verifier.clone(),
            digest,
            requested_scopes: scopes.iter().cloned().collect::<Vec<_>>().join(" "),
        })))
    }
}
