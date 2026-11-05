use super::{PluginConnectionState, PluginConnectionStatus};
use crate::plugins::Plugins;
use crate::{Actor, Error, Result};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use hito_plugin_sdk::{Authorization, Manifest};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::{Row, SqliteConnection};
use std::{collections::BTreeSet, time::Instant};
use zeroize::Zeroizing;

const MAX_LIFETIME_SECONDS: u64 = 31_536_000;
const DEFAULT_LIFETIME_SECONDS: i64 = 3600;
const MINIMUM_LIFETIME_SECONDS: i64 = 120;

#[derive(Serialize)]
struct SealedTokens<'a> {
    access: &'a str,
    refresh: Option<&'a str>,
}

#[derive(Deserialize)]
pub(super) struct OpenedTokens {
    pub(super) access: Zeroizing<String>,
    pub(super) refresh: Option<Zeroizing<String>>,
}

pub(super) struct Stored {
    pub(super) service_id: String,
    pub(super) digest: Vec<u8>,
    pub(super) state: PluginConnectionState,
    pub(super) tokens: Option<Vec<u8>>,
    pub(super) scopes: Vec<String>,
    pub(super) expires_at: Option<i64>,
    pub(super) remaining: Option<i64>,
    pub(super) authorization: Authorization,
}

pub(super) struct NewConnection<'a> {
    pub(super) account: &'a str,
    pub(super) installation: &'a str,
    pub(super) service: &'a str,
    pub(super) digest: &'a [u8],
    pub(super) sealed: &'a [u8],
    pub(super) scopes: &'a str,
    pub(super) lifetime: Option<i64>,
}

pub(super) fn declaration_digest(authorization: &Authorization) -> Result<[u8; 32]> {
    let encoded = serde_json::to_vec(authorization).map_err(|_| {
        Error::Internal("Plugin authorization declaration could not be encoded".into())
    })?;
    Ok(Sha256::digest(encoded).into())
}

pub(super) fn scope_list(granted: Option<&str>) -> Option<String> {
    let scopes: BTreeSet<&str> = granted?
        .split_ascii_whitespace()
        .filter(|scope| scope.len() <= 128 && scope.bytes().all(|byte| byte.is_ascii_graphic()))
        .take(64)
        .collect();
    (!scopes.is_empty()).then(|| scopes.into_iter().collect::<Vec<_>>().join(" "))
}

pub(super) fn lifetime_seconds(expires_in: Option<u64>, refreshable: bool) -> Option<i64> {
    match expires_in {
        Some(0) => Some(MINIMUM_LIFETIME_SECONDS),
        Some(seconds) => Some(seconds.min(MAX_LIFETIME_SECONDS) as i64),
        None => refreshable.then_some(DEFAULT_LIFETIME_SECONDS),
    }
}

pub(super) async fn store(
    connection: &mut SqliteConnection,
    new: &NewConnection<'_>,
) -> Result<()> {
    sqlx::query("INSERT INTO plugin_connections(installation_id,user_id,service_id,declaration_digest,status,tokens,scopes,expires_at)
        VALUES (?1,?2,?3,?4,'connected',?5,?6,unixepoch() + ?7)
        ON CONFLICT(installation_id) DO UPDATE SET user_id = excluded.user_id, service_id = excluded.service_id,
            declaration_digest = excluded.declaration_digest, status = 'connected', tokens = excluded.tokens,
            scopes = excluded.scopes, expires_at = excluded.expires_at, refresh_failures = 0, refresh_failing_since = NULL, connected_at = unixepoch(), updated_at = unixepoch()")
        .bind(new.installation).bind(new.account).bind(new.service).bind(new.digest)
        .bind(new.sealed).bind(new.scopes).bind(new.lifetime)
        .execute(connection).await?;
    Ok(())
}

pub(in crate::plugins) async fn retain_declared(
    connection: &mut SqliteConnection,
    installation: &str,
    authorization: Option<&Authorization>,
) -> Result<()> {
    let removed = match authorization {
        Some(authorization) => {
            let digest = declaration_digest(authorization)?;
            sqlx::query("DELETE FROM plugin_connections WHERE installation_id = ? AND declaration_digest <> ?")
                .bind(installation).bind(&digest[..]).execute(connection).await?.rows_affected()
        }
        None => sqlx::query("DELETE FROM plugin_connections WHERE installation_id = ?")
            .bind(installation)
            .execute(connection)
            .await?
            .rows_affected(),
    };
    if removed > 0 {
        tracing::info!(
            installation_id = installation,
            reason = "declaration_changed",
            "Provider connection invalidated"
        );
    }
    Ok(())
}

impl Plugins {
    pub(super) fn seal_tokens(
        &self,
        account: &str,
        installation: &str,
        service: &str,
        digest: &[u8],
        access: &str,
        refresh: Option<&str>,
    ) -> Result<Vec<u8>> {
        let plaintext = Zeroizing::new(
            serde_json::to_vec(&SealedTokens { access, refresh })
                .map_err(|_| Error::Internal("Provider tokens could not be encoded".into()))?,
        );
        self.vault.seal_connection(
            account,
            installation,
            service,
            &URL_SAFE_NO_PAD.encode(digest),
            &plaintext,
        )
    }

    pub(super) fn open_tokens(
        &self,
        account: &str,
        installation: &str,
        stored: &Stored,
        envelope: &[u8],
    ) -> Result<OpenedTokens> {
        let plaintext = self.vault.reveal_connection(
            account,
            installation,
            &stored.service_id,
            &URL_SAFE_NO_PAD.encode(&stored.digest),
            envelope,
        )?;
        serde_json::from_slice(&plaintext)
            .map_err(|_| Error::Internal("Stored provider tokens are invalid".into()))
    }

    pub(super) async fn load_connection(&self, account: &str, id: &str) -> Result<Option<Stored>> {
        let row = sqlx::query("SELECT p.service_id,p.manifest,c.declaration_digest,c.status,c.tokens,c.scopes,c.expires_at,c.expires_at - unixepoch() AS remaining
            FROM plugin_installations p
            JOIN plugin_services s ON s.id = p.service_id AND s.active = 1
            LEFT JOIN plugin_connections c ON c.installation_id = p.id AND c.user_id = p.user_id
            WHERE p.id = ? AND p.user_id = ? AND p.active = 1")
            .bind(id).bind(account).fetch_optional(&self.store.pool).await?.ok_or(Error::NotFound)?;
        let Some(digest) = row.try_get::<Option<Vec<u8>>, _>("declaration_digest")? else {
            return Ok(None);
        };
        let declared = serde_json::from_str::<Manifest>(row.try_get("manifest")?)
            .ok()
            .filter(|manifest| manifest.validate().is_ok())
            .and_then(|manifest| manifest.authorization);
        let current = declared
            .as_ref()
            .map(declaration_digest)
            .transpose()?
            .filter(|current| current[..] == digest[..]);
        let (Some(authorization), Some(_)) = (declared, current) else {
            let removed = sqlx::query("DELETE FROM plugin_connections WHERE installation_id = ? AND declaration_digest = ?")
                .bind(id).bind(&digest).execute(&self.store.pool).await?.rows_affected();
            if removed > 0 {
                tracing::info!(
                    installation_id = id,
                    reason = "declaration_changed",
                    "Provider connection invalidated"
                );
            }
            return Ok(None);
        };
        let scopes: String = row.try_get("scopes")?;
        Ok(Some(Stored {
            service_id: row.try_get("service_id")?,
            digest,
            state: if row.try_get::<String, _>("status")? == "connected" {
                PluginConnectionState::Connected
            } else {
                PluginConnectionState::NeedsReauthorization
            },
            tokens: row.try_get("tokens")?,
            scopes: scopes
                .split(' ')
                .filter(|scope| !scope.is_empty())
                .map(str::to_owned)
                .collect(),
            expires_at: row.try_get("expires_at")?,
            remaining: row.try_get("remaining")?,
            authorization,
        }))
    }

    #[tracing::instrument(level = "debug", skip_all, err(Debug), fields(account_id = actor.id(), installation_id = id))]
    pub async fn connection_status(
        &self,
        actor: &Actor,
        id: &str,
    ) -> Result<PluginConnectionStatus> {
        let started = Instant::now();
        actor.check_session(&self.store).await?;
        let stored = self.load_connection(actor.id(), id).await?;
        let status = match stored {
            Some(stored) => PluginConnectionStatus {
                status: stored.state,
                expires_at: stored.expires_at,
                scopes: stored.scopes,
            },
            None => PluginConnectionStatus {
                status: PluginConnectionState::NotConnected,
                expires_at: None,
                scopes: Vec::new(),
            },
        };
        tracing::debug!(outcome = ?status.status, elapsed_ms = started.elapsed().as_millis() as u64, "Provider connection inspected");
        Ok(status)
    }

    #[tracing::instrument(level = "info", skip_all, err(Debug), fields(account_id = actor.id(), installation_id = id))]
    pub async fn disconnect(&self, actor: &Actor, id: &str) -> Result<()> {
        let started = Instant::now();
        let mut tx = self.store.pool.begin_with("BEGIN IMMEDIATE").await?;
        actor.check_session_on(&mut tx).await?;
        sqlx::query_scalar::<_, i64>(
            "SELECT 1 FROM plugin_installations WHERE id = ? AND user_id = ? AND active = 1",
        )
        .bind(id)
        .bind(actor.id())
        .fetch_optional(&mut *tx)
        .await?
        .ok_or(Error::NotFound)?;
        let removed =
            sqlx::query("DELETE FROM plugin_connections WHERE installation_id = ? AND user_id = ?")
                .bind(id)
                .bind(actor.id())
                .execute(&mut *tx)
                .await?
                .rows_affected();
        let closed = sqlx::query("UPDATE plugin_authorization_attempts SET status = 'failed', pending_secret = zeroblob(74)
            WHERE installation_id = ? AND user_id = ? AND status IN ('pending','exchanging')")
            .bind(id).bind(actor.id()).execute(&mut *tx).await?.rows_affected();
        tx.commit().await?;
        tracing::info!(
            outcome = if removed == 1 {
                "disconnected"
            } else {
                "not_connected"
            },
            attempts_closed = closed,
            elapsed_ms = started.elapsed().as_millis() as u64,
            "Provider connection disconnected"
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_declaration_digest_encoding_is_pinned() {
        let authorization = Authorization::Oauth2Pkce {
            client_id: "owned-pin-client".into(),
            authorization_endpoint: "https://provider.example/authorize".into(),
            token_endpoint: "https://provider.example/token".into(),
            scopes: ["b".to_owned(), "a".to_owned()].into(),
        };
        assert_eq!(
            serde_json::to_string(&authorization).unwrap(),
            r#"{"kind":"oauth2_pkce","client_id":"owned-pin-client","authorization_endpoint":"https://provider.example/authorize","token_endpoint":"https://provider.example/token","scopes":["a","b"]}"#
        );
        let digest = declaration_digest(&authorization).unwrap();
        let hex: String = digest.iter().map(|byte| format!("{byte:02x}")).collect();
        assert_eq!(
            hex,
            "a81a938f64a37cdcdba5bf176786fcdcbc7a432bfee9247ac777777fa3ce1cec"
        );
    }

    #[test]
    fn lifetimes_default_when_refreshable_and_never_collapse_to_zero() {
        assert_eq!(lifetime_seconds(None, true), Some(3600));
        assert_eq!(lifetime_seconds(None, false), None);
        assert_eq!(lifetime_seconds(Some(0), true), Some(120));
        assert_eq!(lifetime_seconds(Some(0), false), Some(120));
        assert_eq!(lifetime_seconds(Some(30), true), Some(30));
        assert_eq!(lifetime_seconds(Some(u64::MAX), true), Some(31_536_000));
    }
}
