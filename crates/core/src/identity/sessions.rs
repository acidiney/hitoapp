use super::{Actor, AuthSession, Credentials, Identity, SESSION_SECONDS, User, credentials};
use crate::{Error, Result};
use sha2::{Digest, Sha256};
use tracing::Instrument;
use uuid::Uuid;

const SEEN_INTERVAL_SECONDS: i64 = 300;
const SEEN_CONNECTION_WAIT: std::time::Duration = std::time::Duration::from_millis(500);
const SEEN_LOCK_WAIT_MILLIS: i64 = 100;
const SEEN_RETRY_AFTER: std::time::Duration = std::time::Duration::from_secs(2);
const MAX_CLIENT_BYTES: usize = 64;

#[derive(Clone, Default)]
pub(crate) struct SeenAttempts {
    sessions:
        std::sync::Arc<std::sync::Mutex<std::collections::HashMap<String, std::time::Instant>>>,
}

impl SeenAttempts {
    fn reserve(&self, session_hash: &str) -> bool {
        let now = std::time::Instant::now();
        let mut sessions = self
            .sessions
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        sessions.retain(|_, since| now.duration_since(*since) < SEEN_RETRY_AFTER);
        sessions.insert(session_hash.to_owned(), now).is_none()
    }
}

async fn write_seen(
    pool: &sqlx::SqlitePool,
    session_hash: &str,
) -> std::result::Result<(), &'static str> {
    let mut connection = tokio::time::timeout(SEEN_CONNECTION_WAIT, pool.acquire())
        .await
        .map_err(|_| "timeout")?
        .map_err(|_| "storage")?;
    let patience: i64 = sqlx::query_scalar("PRAGMA busy_timeout")
        .fetch_one(&mut *connection)
        .await
        .map_err(|_| "storage")?;
    sqlx::query(&format!("PRAGMA busy_timeout = {SEEN_LOCK_WAIT_MILLIS}"))
        .execute(&mut *connection)
        .await
        .map_err(|_| "storage")?;
    let written = sqlx::query(
        "UPDATE sessions SET last_seen_at = unixepoch()
         WHERE token_hash = ? AND unixepoch() - last_seen_at >= ?",
    )
    .bind(session_hash)
    .bind(SEEN_INTERVAL_SECONDS)
    .execute(&mut *connection)
    .await;
    if sqlx::query(&format!("PRAGMA busy_timeout = {patience}"))
        .execute(&mut *connection)
        .await
        .is_err()
    {
        connection.close_on_drop();
    }
    written.map(|_| ()).map_err(|_| "storage")
}

pub(super) fn is_hidden(character: char) -> bool {
    character.is_control()
        || matches!(
            character,
            '\u{00ad}'
                | '\u{061c}'
                | '\u{180e}'
                | '\u{200b}'..='\u{200f}'
                | '\u{2028}'..='\u{202e}'
                | '\u{2060}'..='\u{206f}'
                | '\u{feff}'
                | '\u{fff9}'..='\u{fffb}'
        )
}

fn clean_client(client: Option<&str>) -> Option<String> {
    let mut cleaned = String::new();
    for character in client?.trim().chars().filter(|c| !is_hidden(*c)) {
        if cleaned.len() + character.len_utf8() > MAX_CLIENT_BYTES {
            break;
        }
        cleaned.push(character);
    }
    let cleaned = cleaned.trim();
    (!cleaned.is_empty()).then(|| cleaned.to_owned())
}

impl Identity {
    #[tracing::instrument(level = "trace", skip_all, err(Debug), fields(account_id = actor.id()))]
    pub async fn check_session(&self, actor: &Actor) -> Result<()> {
        tokio::time::timeout(
            std::time::Duration::from_secs(1),
            actor.check_session(&self.store),
        )
        .await
        .unwrap_or(Err(Error::Upstream(
            "Session authorization could not be checked in time",
        )))
    }

    pub async fn login(&self, credentials: Credentials) -> Result<AuthSession> {
        self.login_with_client(credentials, None).await
    }

    #[tracing::instrument(level = "info", skip_all, err(Debug))]
    pub async fn login_with_client(
        &self,
        credentials: Credentials,
        client: Option<&str>,
    ) -> Result<AuthSession> {
        if credentials.password.len() > 1024 || credentials.username.len() > 64 {
            return Err(Error::Unauthorized);
        }
        let row: Option<(String, String, bool, Option<String>, String)> = sqlx::query_as(
            "SELECT id, username, admin, display_name, password_hash FROM users WHERE username = ?",
        )
        .bind(credentials.username.trim())
        .fetch_optional(&self.store.pool)
        .await?;
        let Some((id, username, admin, display_name, hash)) = row else {
            return Err(Error::Unauthorized);
        };
        let hash_snapshot = hash.clone();
        if !credentials::verify(credentials.password, hash).await? {
            return Err(Error::Unauthorized);
        }
        self.create_session(
            User {
                id,
                username,
                admin,
                display_name,
            },
            Some(&hash_snapshot),
            client,
        )
        .await
    }

    #[tracing::instrument(level = "debug", skip_all, err(Debug))]
    pub(super) async fn create_session(
        &self,
        user: User,
        verified_hash: Option<&str>,
        client: Option<&str>,
    ) -> Result<AuthSession> {
        let token = format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
        let inserted = sqlx::query(
            "INSERT INTO sessions(token_hash, user_id, expires_at, created_at, last_seen_at, client)
             SELECT ?1, ?2, unixepoch() + ?3, unixepoch(), unixepoch(), ?5
             WHERE ?4 IS NULL OR EXISTS(SELECT 1 FROM users WHERE id = ?2 AND password_hash = ?4)",
        )
        .bind(token_hash(&token))
        .bind(&user.id)
        .bind(SESSION_SECONDS)
        .bind(verified_hash)
        .bind(clean_client(client))
        .execute(&self.store.pool)
        .await?
        .rows_affected();
        if inserted == 0 {
            return Err(Error::Unauthorized);
        }
        tracing::info!(account_id = %user.id, "Session created");
        Ok(AuthSession { token, user })
    }

    #[tracing::instrument(level = "debug", skip_all, err(Debug))]
    pub async fn authenticate(&self, token: &str) -> Result<Actor> {
        let session_hash = token_hash(token);
        let (id, username, admin, display_name, quiet_seconds): (
            String,
            String,
            bool,
            Option<String>,
            i64,
        ) = sqlx::query_as(
            "SELECT u.id, u.username, u.admin, u.display_name, unixepoch() - s.last_seen_at
             FROM users u JOIN sessions s ON s.user_id = u.id
             WHERE s.token_hash = ? AND s.expires_at > unixepoch()",
        )
        .bind(&session_hash)
        .fetch_optional(&self.store.pool)
        .await?
        .ok_or(Error::Unauthorized)?;
        let user = User {
            id,
            username,
            admin,
            display_name,
        };
        if quiet_seconds >= SEEN_INTERVAL_SECONDS {
            self.record_seen(&session_hash);
        }
        let locked = self.session_locked(&user.id, &session_hash).await?;
        Ok(Actor {
            user,
            session_hash,
            locked,
            background: false,
            unattended: false,
        })
    }

    fn record_seen(&self, session_hash: &str) {
        if !self.seen_attempts.reserve(session_hash) {
            return;
        }
        let pool = self.store.pool.clone();
        let session_hash = session_hash.to_owned();
        tokio::spawn(
            async move {
                let Err(error_kind) = write_seen(&pool, &session_hash).await else {
                    return;
                };
                tracing::debug!(
                    event = "session_seen_failed",
                    error_kind,
                    "Session last-seen time was not recorded"
                );
            }
            .in_current_span(),
        );
    }

    #[tracing::instrument(level = "info", skip_all, err(Debug))]
    pub async fn logout(&self, token: &str) -> Result<()> {
        sqlx::query("DELETE FROM sessions WHERE token_hash = ?")
            .bind(token_hash(token))
            .execute(&self.store.pool)
            .await?;
        tracing::info!("Session revoked");
        Ok(())
    }
}

impl Actor {
    pub(crate) async fn resume_session_on(
        connection: &mut sqlx::SqliteConnection,
        account: &str,
        session_hash: &str,
    ) -> Result<Self> {
        let user = sqlx::query_as(
            "SELECT u.id,u.username,u.admin,u.display_name FROM users u JOIN sessions s ON s.user_id = u.id
             WHERE s.user_id = ? AND s.token_hash = ? AND s.expires_at > unixepoch()",
        )
        .bind(account)
        .bind(session_hash)
        .fetch_optional(&mut *connection)
        .await?
        .ok_or(Error::Unauthorized)?;
        let locked = sqlx::query_scalar("SELECT locked FROM session_locks WHERE token_hash = ?")
            .bind(session_hash)
            .fetch_optional(connection)
            .await?
            .unwrap_or(false);
        Ok(Self {
            user,
            session_hash: session_hash.to_owned(),
            locked,
            background: true,
            unattended: false,
        })
    }

    pub(crate) async fn unattended_on(
        connection: &mut sqlx::SqliteConnection,
        account: &str,
    ) -> Result<Self> {
        let user = sqlx::query_as("SELECT id,username,admin,display_name FROM users WHERE id = ?")
            .bind(account)
            .fetch_optional(connection)
            .await?
            .ok_or(Error::NotFound)?;
        Ok(Self {
            user,
            session_hash: String::new(),
            locked: false,
            background: true,
            unattended: true,
        })
    }

    pub(crate) async fn check_session(&self, store: &crate::Store) -> Result<()> {
        self.check_session_on(&mut *store.pool.acquire().await?)
            .await
    }

    pub(crate) async fn check_session_on(
        &self,
        connection: &mut sqlx::SqliteConnection,
    ) -> Result<()> {
        if self.unattended {
            let exists: bool =
                sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM users WHERE id = ?)")
                    .bind(self.id())
                    .fetch_one(connection)
                    .await?;
            return if exists {
                Ok(())
            } else {
                Err(Error::Unauthorized)
            };
        }
        let (valid, locked): (bool, bool) = sqlx::query_as(
            "SELECT EXISTS(SELECT 1 FROM sessions WHERE token_hash = ?1 AND user_id = ?2 AND expires_at > unixepoch()),
                    EXISTS(SELECT 1 FROM account_pin_locks p LEFT JOIN session_locks l ON l.token_hash = ?1
                           WHERE p.user_id = ?2 AND (COALESCE(l.locked, 0) = 1
                               OR (p.idle_minutes > 0 AND unixepoch() - COALESCE(l.active_at, unixepoch()) > p.idle_minutes * 60)))",
        )
        .bind(&self.session_hash)
        .bind(self.id())
        .fetch_one(connection)
        .await?;
        if !valid {
            Err(Error::Unauthorized)
        } else if locked && !self.background {
            Err(Error::Locked)
        } else {
            Ok(())
        }
    }

    pub(crate) async fn check_session_exists_on(
        &self,
        connection: &mut sqlx::SqliteConnection,
    ) -> Result<()> {
        if self.unattended {
            return self.check_session_on(connection).await;
        }
        let valid: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM sessions WHERE token_hash = ? AND user_id = ? AND expires_at > unixepoch())")
            .bind(&self.session_hash).bind(self.id()).fetch_one(connection).await?;
        if valid {
            Ok(())
        } else {
            Err(Error::Unauthorized)
        }
    }
}

fn token_hash(token: &str) -> String {
    format!("{:x}", Sha256::digest(token.as_bytes()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_write_attempt_is_pending_per_session_inside_the_retry_window() {
        let attempts = SeenAttempts::default();
        assert!(attempts.reserve("a"));
        for _ in 0..50 {
            assert!(!attempts.reserve("a"));
        }
        assert!(attempts.reserve("b"));
    }

    #[test]
    fn hidden_and_format_characters_are_removed_from_labels() {
        assert_eq!(
            clean_client(Some(
                "Fire\u{202e}fox\u{200b} on\u{2028}mac\u{feff}\u{2066}"
            ))
            .as_deref(),
            Some("Firefox onmac")
        );
        assert_eq!(clean_client(Some("\u{202e}\u{200b}")), None);
    }
}
