use super::{
    PluginConnectionState, ProviderAccess,
    connection::{Stored, scope_list},
    exchange::{ExchangeFailure, Refresh},
};
use crate::plugins::Plugins;
use crate::{Actor, Error, Result};
use hito_plugin_sdk::Authorization;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex, PoisonError, Weak},
    time::Instant,
};

const SKEW_SECONDS: i64 = 60;

#[derive(Default)]
pub(crate) struct ConnectionLocks(Mutex<HashMap<String, Weak<tokio::sync::Mutex<()>>>>);

impl ConnectionLocks {
    fn lock_for(&self, installation: &str) -> Arc<tokio::sync::Mutex<()>> {
        let mut locks = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        locks.retain(|_, lock| lock.strong_count() > 0);
        if let Some(lock) = locks.get(installation).and_then(Weak::upgrade) {
            return lock;
        }
        let lock = Arc::new(tokio::sync::Mutex::new(()));
        locks.insert(installation.to_owned(), Arc::downgrade(&lock));
        lock
    }
}

fn required(message: &'static str) -> Error {
    Error::AuthorizationRequired(message)
}

fn fresh(stored: &Stored) -> bool {
    stored
        .remaining
        .is_none_or(|remaining| remaining > SKEW_SECONDS)
}

impl Plugins {
    #[tracing::instrument(level = "debug", skip_all, err(Debug), fields(account_id = actor.id(), installation_id = id))]
    pub async fn access_token(&self, actor: &Actor, id: &str) -> Result<ProviderAccess> {
        let started = Instant::now();
        actor.check_session(&self.store).await?;
        let stored = self.usable(actor, id).await?;
        if fresh(&stored) {
            return self.serve(actor, id, &stored, "fresh", started);
        }
        let lock = self.connection_locks.lock_for(id);
        let _single_flight = lock.lock().await;
        let stored = self.usable(actor, id).await?;
        if fresh(&stored) {
            return self.serve(actor, id, &stored, "fresh", started);
        }
        self.renew_access(actor, id, stored, started).await
    }

    async fn usable(&self, actor: &Actor, id: &str) -> Result<Stored> {
        let stored = self
            .load_connection(actor, id)
            .await?
            .ok_or_else(|| required("Connect your provider account to continue"))?;
        if stored.state != PluginConnectionState::Connected || stored.tokens.is_none() {
            return Err(required("Reconnect your provider account to continue"));
        }
        Ok(stored)
    }

    fn serve(
        &self,
        actor: &Actor,
        id: &str,
        stored: &Stored,
        outcome: &'static str,
        started: Instant,
    ) -> Result<ProviderAccess> {
        let envelope = stored.tokens.as_deref().unwrap_or_default();
        let opened = self.open_tokens(actor.id(), id, stored, envelope)?;
        tracing::debug!(
            outcome,
            elapsed_ms = started.elapsed().as_millis() as u64,
            "Provider access token served"
        );
        Ok(ProviderAccess {
            token: opened.access,
            expires_at: stored.expires_at,
        })
    }

    async fn renew_access(
        &self,
        actor: &Actor,
        id: &str,
        stored: Stored,
        started: Instant,
    ) -> Result<ProviderAccess> {
        let envelope = stored.tokens.clone().unwrap_or_default();
        let opened = match self.open_tokens(actor.id(), id, &stored, &envelope) {
            Ok(opened) => opened,
            Err(error) if !self.vault.ready() => return Err(error),
            Err(_) => {
                self.require_reauthorization(actor, id, &envelope, "unreadable", started)
                    .await?;
                return Err(required("Reconnect your provider account to continue"));
            }
        };
        let Some(refresh_token) = opened.refresh.as_ref() else {
            self.require_reauthorization(actor, id, &envelope, "no_refresh_token", started)
                .await?;
            return Err(required("Reconnect your provider account to continue"));
        };
        let Authorization::Oauth2Pkce {
            client_id,
            token_endpoint,
            ..
        } = &stored.authorization;
        let admission = self.token.admit(actor.id())?;
        let renewed = self
            .token
            .renew(Refresh {
                endpoint: token_endpoint,
                client_id,
                refresh_token,
            })
            .await;
        drop(admission);
        let grant = match renewed {
            Ok(grant) => grant,
            Err(ExchangeFailure::InvalidGrant) => {
                self.require_reauthorization(actor, id, &envelope, "invalid_grant", started)
                    .await?;
                return Err(required("Reconnect your provider account to continue"));
            }
            Err(failure) => {
                tracing::warn!(
                    outcome = "retryable",
                    reason = failure.as_str(),
                    elapsed_ms = started.elapsed().as_millis() as u64,
                    "Provider connection refresh failed"
                );
                return Err(Error::Upstream(
                    "The provider could not refresh this connection. Try again later.",
                ));
            }
        };
        let rotated = grant.refresh_token.is_some();
        let sealed = self.seal_tokens(
            actor.id(),
            id,
            &stored.service_id,
            &stored.digest,
            &grant.access_token,
            Some(
                grant
                    .refresh_token
                    .as_deref()
                    .map_or(refresh_token.as_str(), String::as_str),
            ),
        )?;
        let scopes = scope_list(grant.scope.as_deref());
        let lifetime = grant
            .expires_in
            .map(|seconds| seconds.min(31_536_000) as i64);
        let updated: Option<Option<i64>> = sqlx::query_scalar("UPDATE plugin_connections SET tokens = ?, expires_at = unixepoch() + ?, scopes = COALESCE(?, scopes), updated_at = unixepoch()
            WHERE installation_id = ? AND user_id = ? AND status = 'connected' AND tokens = ? RETURNING expires_at")
            .bind(&sealed).bind(lifetime).bind(scopes).bind(id).bind(actor.id()).bind(&envelope)
            .fetch_optional(&self.store.pool).await?;
        let Some(expires_at) = updated else {
            let current = self.usable(actor, id).await?;
            if fresh(&current) {
                return self.serve(actor, id, &current, "fresh", started);
            }
            return Err(Error::ProviderBusy(
                "This provider connection changed. Try again in a moment.",
            ));
        };
        tracing::info!(
            installation_id = id,
            outcome = "refreshed",
            rotated,
            elapsed_ms = started.elapsed().as_millis() as u64,
            "Provider connection refreshed"
        );
        tracing::debug!(
            outcome = "refreshed",
            elapsed_ms = started.elapsed().as_millis() as u64,
            "Provider access token served"
        );
        Ok(ProviderAccess {
            token: grant.access_token,
            expires_at,
        })
    }

    async fn require_reauthorization(
        &self,
        actor: &Actor,
        id: &str,
        envelope: &[u8],
        reason: &'static str,
        started: Instant,
    ) -> Result<()> {
        sqlx::query("UPDATE plugin_connections SET status = 'needs_reauthorization', tokens = NULL, expires_at = NULL, updated_at = unixepoch()
            WHERE installation_id = ? AND user_id = ? AND tokens = ?")
            .bind(id).bind(actor.id()).bind(envelope).execute(&self.store.pool).await?;
        tracing::warn!(
            installation_id = id,
            outcome = "needs_reauthorization",
            reason,
            elapsed_ms = started.elapsed().as_millis() as u64,
            "Provider connection needs reauthorization"
        );
        Ok(())
    }
}
