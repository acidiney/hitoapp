use super::{
    PluginConnectionState, ProviderAccess,
    connection::{OpenedTokens, Stored, lifetime_seconds, scope_list},
    exchange::{ExchangeFailure, Refresh},
};
use crate::plugins::Plugins;
use crate::{Actor, Error, Result};
use hito_plugin_sdk::Authorization;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex, PoisonError, Weak},
    time::{Duration, Instant},
};
use tracing::{Instrument, instrument::WithSubscriber};

const SKEW_SECONDS: i64 = 60;
const LOCK_WAIT: Duration = Duration::from_secs(15);
const FAILURES_BEFORE_REAUTHORIZATION: i64 = 3;
const FAILING_SECONDS_BEFORE_REAUTHORIZATION: i64 = 86_400;

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

fn reconnect() -> Error {
    required("Reconnect your provider account to continue")
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
        let stored = self.usable(actor.id(), id).await?;
        if fresh(&stored) {
            return self.serve(actor.id(), id, &stored, "fresh", started).await;
        }
        let plugins = self.clone();
        let account = actor.id().to_owned();
        let installation = id.to_owned();
        tokio::spawn(
            async move {
                plugins
                    .refresh_exclusive(&account, &installation, started)
                    .await
            }
            .in_current_span()
            .with_current_subscriber(),
        )
        .await
        .map_err(|_| Error::Internal("Provider connection refresh stopped".into()))?
    }

    async fn refresh_exclusive(
        &self,
        account: &str,
        id: &str,
        started: Instant,
    ) -> Result<ProviderAccess> {
        let lock = self.connection_locks.lock_for(id);
        let _single_flight = tokio::time::timeout(LOCK_WAIT, lock.lock())
            .await
            .map_err(|_| {
                Error::ProviderBusy("This provider connection is busy. Try again in a moment.")
            })?;
        let stored = self.usable(account, id).await?;
        if fresh(&stored) {
            return self.serve(account, id, &stored, "fresh", started).await;
        }
        self.renew_access(account, id, stored, started).await
    }

    async fn usable(&self, account: &str, id: &str) -> Result<Stored> {
        let stored = self
            .load_connection(account, id)
            .await?
            .ok_or_else(|| required("Connect your provider account to continue"))?;
        if stored.state != PluginConnectionState::Connected || stored.tokens.is_none() {
            return Err(reconnect());
        }
        Ok(stored)
    }

    async fn open_or_invalidate(
        &self,
        account: &str,
        id: &str,
        stored: &Stored,
        started: Instant,
    ) -> Result<OpenedTokens> {
        let envelope = stored.tokens.as_deref().unwrap_or_default();
        match self.open_tokens(account, id, stored, envelope) {
            Ok(opened) => Ok(opened),
            Err(error) if !self.vault.ready() => Err(error),
            Err(_) => Err(self
                .invalidate(account, id, envelope, "unreadable", started)
                .await?),
        }
    }

    async fn serve(
        &self,
        account: &str,
        id: &str,
        stored: &Stored,
        outcome: &'static str,
        started: Instant,
    ) -> Result<ProviderAccess> {
        let opened = match self.open_or_invalidate(account, id, stored, started).await {
            Ok(opened) => opened,
            Err(Error::ProviderBusy(_)) => return self.read_current(account, id, started).await,
            Err(error) => return Err(error),
        };
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

    async fn read_current(
        &self,
        account: &str,
        id: &str,
        started: Instant,
    ) -> Result<ProviderAccess> {
        let current = self.usable(account, id).await?;
        if !fresh(&current) {
            return Err(Error::ProviderBusy(
                "This provider connection changed. Try again in a moment.",
            ));
        }
        let envelope = current.tokens.as_deref().unwrap_or_default();
        let opened = self.open_tokens(account, id, &current, envelope)?;
        tracing::debug!(
            outcome = "current",
            elapsed_ms = started.elapsed().as_millis() as u64,
            "Provider access token served"
        );
        Ok(ProviderAccess {
            token: opened.access,
            expires_at: current.expires_at,
        })
    }

    async fn renew_access(
        &self,
        account: &str,
        id: &str,
        stored: Stored,
        started: Instant,
    ) -> Result<ProviderAccess> {
        let envelope = stored.tokens.clone().unwrap_or_default();
        let opened = match self.open_or_invalidate(account, id, &stored, started).await {
            Ok(opened) => opened,
            Err(Error::ProviderBusy(_)) => return self.read_current(account, id, started).await,
            Err(error) => return Err(error),
        };
        let Some(refresh_token) = opened.refresh.as_ref() else {
            return self
                .conclude_invalid(account, id, &envelope, "no_refresh_token", started)
                .await;
        };
        let Authorization::Oauth2Pkce {
            client_id,
            token_endpoint,
            ..
        } = &stored.authorization;
        let admission = self.token.admit(id)?;
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
            Err(failure @ (ExchangeFailure::InvalidGrant | ExchangeFailure::Terminal)) => {
                return self
                    .conclude_invalid(account, id, &envelope, failure.as_str(), started)
                    .await;
            }
            Err(failure @ (ExchangeFailure::Rejected | ExchangeFailure::Invalid)) => {
                if self.record_failure(account, id, &envelope).await? {
                    return self
                        .conclude_invalid(account, id, &envelope, "repeated_failures", started)
                        .await;
                }
                return Err(self.retryable(failure, started));
            }
            Err(failure) => return Err(self.retryable(failure, started)),
        };
        let rotated = grant.refresh_token.is_some();
        let kept = grant
            .refresh_token
            .as_deref()
            .map_or(refresh_token.as_str(), String::as_str);
        let sealed = self.seal_tokens(
            account,
            id,
            &stored.service_id,
            &stored.digest,
            &grant.access_token,
            Some(kept),
        )?;
        let scopes = scope_list(grant.scope.as_deref());
        let lifetime = lifetime_seconds(grant.expires_in, true);
        let updated: Option<Option<i64>> = sqlx::query_scalar("UPDATE plugin_connections SET tokens = ?, expires_at = unixepoch() + ?, scopes = COALESCE(?, scopes),
                refresh_failures = 0, refresh_failing_since = NULL, updated_at = unixepoch()
            WHERE installation_id = ? AND user_id = ? AND status = 'connected' AND tokens = ? RETURNING expires_at")
            .bind(&sealed).bind(lifetime).bind(scopes).bind(id).bind(account).bind(&envelope)
            .fetch_optional(&self.store.pool).await?;
        let Some(expires_at) = updated else {
            return self.read_current(account, id, started).await;
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

    fn retryable(&self, failure: ExchangeFailure, started: Instant) -> Error {
        tracing::warn!(
            outcome = "retryable",
            reason = failure.as_str(),
            elapsed_ms = started.elapsed().as_millis() as u64,
            "Provider connection refresh failed"
        );
        Error::Upstream("The provider could not refresh this connection. Try again later.")
    }

    async fn record_failure(&self, account: &str, id: &str, envelope: &[u8]) -> Result<bool> {
        let exhausted: Option<bool> = sqlx::query_scalar("UPDATE plugin_connections
            SET refresh_failures = refresh_failures + 1, refresh_failing_since = COALESCE(refresh_failing_since, unixepoch())
            WHERE installation_id = ? AND user_id = ? AND tokens = ?
            RETURNING refresh_failures >= ? OR unixepoch() - refresh_failing_since >= ?")
            .bind(id).bind(account).bind(envelope)
            .bind(FAILURES_BEFORE_REAUTHORIZATION).bind(FAILING_SECONDS_BEFORE_REAUTHORIZATION)
            .fetch_optional(&self.store.pool).await?;
        Ok(exhausted.unwrap_or(false))
    }

    async fn conclude_invalid(
        &self,
        account: &str,
        id: &str,
        envelope: &[u8],
        reason: &'static str,
        started: Instant,
    ) -> Result<ProviderAccess> {
        match self
            .invalidate(account, id, envelope, reason, started)
            .await?
        {
            Error::AuthorizationRequired(_) => Err(reconnect()),
            _ => self.read_current(account, id, started).await,
        }
    }

    async fn invalidate(
        &self,
        account: &str,
        id: &str,
        envelope: &[u8],
        reason: &'static str,
        started: Instant,
    ) -> Result<Error> {
        let changed = sqlx::query("UPDATE plugin_connections SET status = 'needs_reauthorization', tokens = NULL, expires_at = NULL, updated_at = unixepoch()
            WHERE installation_id = ? AND user_id = ? AND tokens = ?")
            .bind(id).bind(account).bind(envelope).execute(&self.store.pool).await?.rows_affected();
        if changed == 0 {
            return Ok(Error::ProviderBusy("changed"));
        }
        tracing::warn!(
            installation_id = id,
            outcome = "needs_reauthorization",
            reason,
            elapsed_ms = started.elapsed().as_millis() as u64,
            "Provider connection needs reauthorization"
        );
        Ok(reconnect())
    }
}
