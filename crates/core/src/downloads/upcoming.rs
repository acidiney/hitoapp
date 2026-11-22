use super::Downloads;
use crate::{Actor, Error, Result, identity::recording_access};
use serde::{Deserialize, Serialize};
use sqlx::SqliteConnection;
use std::{
    collections::{HashMap, VecDeque},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use utoipa::ToSchema;

pub const MAX_UPCOMING_RECORDINGS: usize = 2;
const PACE_WINDOW: Duration = Duration::from_secs(60);
const PACE_LIMIT: usize = 30;
const PACE_ACCOUNTS: usize = 4096;

#[derive(Debug, Deserialize, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct UpcomingRecordings {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(min_length = 1, max_length = 64)]
    pub client: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current: Option<String>,
    #[schema(max_items = 2)]
    pub recording_ids: Vec<String>,
}

#[derive(Clone, Default)]
pub(crate) struct AnnouncementPace(Arc<Mutex<HashMap<String, VecDeque<Instant>>>>);

impl AnnouncementPace {
    fn admit(&self, account: &str) -> Result<()> {
        let now = Instant::now();
        let mut accounts = self
            .0
            .lock()
            .map_err(|_| Error::Internal("Upcoming track admission is unavailable".into()))?;
        for announced in accounts.values_mut() {
            while announced
                .front()
                .is_some_and(|time| now.duration_since(*time) >= PACE_WINDOW)
            {
                announced.pop_front();
            }
        }
        accounts.retain(|_, announced| !announced.is_empty());
        if accounts.len() >= PACE_ACCOUNTS && !accounts.contains_key(account) {
            return Err(Error::RateLimited(1));
        }
        let announced = accounts.entry(account.to_owned()).or_default();
        if announced.len() >= PACE_LIMIT {
            let waited = announced
                .front()
                .map_or(Duration::ZERO, |first| now.duration_since(*first));
            return Err(Error::RateLimited(
                PACE_WINDOW.saturating_sub(waited).as_secs().max(1),
            ));
        }
        announced.push_back(now);
        Ok(())
    }
}

impl Downloads {
    #[tracing::instrument(level = "debug", skip_all, err(Debug), fields(account_id = actor.id()))]
    pub async fn announce_upcoming(
        &self,
        actor: &Actor,
        announcement: UpcomingRecordings,
    ) -> Result<()> {
        actor.require_unlocked()?;
        actor.require_session()?;
        let client = announcement.client.clone().unwrap_or_default();
        if announcement.client.is_some() && !valid_client(&client) {
            return Err(Error::Invalid(
                "Use a client tag of 1 to 64 letters, digits, dashes, dots, colons or underscores"
                    .into(),
            ));
        }
        if announcement.recording_ids.len() > MAX_UPCOMING_RECORDINGS {
            return Err(Error::Invalid(
                "Announce at most 2 upcoming recordings".into(),
            ));
        }
        let current = announcement
            .current
            .as_deref()
            .map(recording_identity)
            .transpose()?;
        let mut recordings: Vec<String> = Vec::with_capacity(MAX_UPCOMING_RECORDINGS);
        for recording in &announcement.recording_ids {
            let recording = recording_identity(recording)?;
            if !recordings.contains(&recording) {
                recordings.push(recording);
            }
        }
        self.announcements.admit(actor.id())?;
        let started = Instant::now();
        let mut tx = self.store.pool.begin_with("BEGIN IMMEDIATE").await?;
        actor.check_session_on(&mut tx).await?;
        let announced = recordings.len();
        let mut accepted = Vec::with_capacity(announced);
        for recording in recordings {
            match recording_access(&mut tx, actor, &recording).await {
                Ok(()) => accepted.push(recording),
                Err(Error::NotFound) => {}
                Err(error) => return Err(error),
            }
        }
        sqlx::query("DELETE FROM upcoming_recordings WHERE announced_at<=unixepoch()-?")
            .bind(super::eviction::UPCOMING_SECONDS)
            .execute(&mut *tx)
            .await?;
        let listed = serde_json::to_string(&accepted)
            .map_err(|_| Error::Internal("Upcoming recordings could not be encoded".into()))?;
        sqlx::query(
            "DELETE FROM upcoming_recordings WHERE session_hash=? AND client=?
             AND recording_id NOT IN (SELECT value FROM json_each(?))",
        )
        .bind(actor.session_binding())
        .bind(&client)
        .bind(listed)
        .execute(&mut *tx)
        .await?;
        for (position, recording) in accepted.iter().enumerate() {
            sqlx::query(
                "INSERT INTO upcoming_recordings(session_hash,client,account_id,recording_id,position) VALUES(?,?,?,?,?)
                 ON CONFLICT(session_hash,client,recording_id) DO UPDATE SET position=excluded.position,announced_at=unixepoch()",
            )
            .bind(actor.session_binding())
            .bind(&client)
            .bind(actor.id())
            .bind(recording)
            .bind(position as i64)
            .execute(&mut *tx)
            .await?;
        }
        let playing = announce_current_on(&mut tx, actor, &client, current).await?;
        tx.commit().await?;
        tracing::debug!(
            event = "upcoming_recordings_announced",
            accepted = accepted.len(),
            ignored = announced - accepted.len(),
            playing,
            elapsed_ms = started.elapsed().as_millis() as u64,
            "Upcoming recordings announced"
        );
        Ok(())
    }
}

async fn announce_current_on(
    connection: &mut SqliteConnection,
    actor: &Actor,
    client: &str,
    current: Option<String>,
) -> Result<bool> {
    sqlx::query("DELETE FROM playing_recordings WHERE announced_at<=unixepoch()-?")
        .bind(super::eviction::UPCOMING_SECONDS)
        .execute(&mut *connection)
        .await?;
    let accepted = match current {
        Some(recording) => match recording_access(&mut *connection, actor, &recording).await {
            Ok(()) => Some(recording),
            Err(Error::NotFound) => None,
            Err(error) => return Err(error),
        },
        None => None,
    };
    let Some(recording) = accepted else {
        sqlx::query("DELETE FROM playing_recordings WHERE session_hash=? AND client=?")
            .bind(actor.session_binding())
            .bind(client)
            .execute(connection)
            .await?;
        return Ok(false);
    };
    sqlx::query(
        "INSERT INTO playing_recordings(session_hash,client,account_id,recording_id) VALUES(?,?,?,?)
         ON CONFLICT(session_hash,client) DO UPDATE SET recording_id=excluded.recording_id,announced_at=unixepoch(),
            next_attempt_at=CASE WHEN playing_recordings.recording_id=excluded.recording_id
                THEN playing_recordings.next_attempt_at ELSE 0 END",
    )
    .bind(actor.session_binding())
    .bind(client)
    .bind(actor.id())
    .bind(recording)
    .execute(connection)
    .await?;
    Ok(true)
}

fn recording_identity(recording: &str) -> Result<String> {
    uuid::Uuid::parse_str(recording)
        .map(|recording| recording.to_string())
        .map_err(|_| Error::Invalid("Use recording identities".into()))
}

fn valid_client(client: &str) -> bool {
    !client.is_empty()
        && client.len() <= 64
        && client
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"-_.:".contains(&byte))
}
