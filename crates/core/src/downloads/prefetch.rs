use super::{
    Downloads, OriginSelection,
    copies::preference_on,
    eviction::{Room, UPCOMING_SECONDS},
    lease::Claim,
    retention::Admission,
};
use crate::{Actor, Error, Result};
use sqlx::SqliteConnection;
use std::{future::Future, time::Instant};
use tokio::sync::watch;

const LOW_DISK_PERCENT: u64 = 10;
const FAILED_RETRY_SECONDS: i64 = 60 * 60;
const BUSY_RETRY_SECONDS: i64 = 60;
const NO_SOURCE_RETRY_SECONDS: i64 = 10 * 60;
const OVER_BUDGET_RETRY_SECONDS: i64 = 10 * 60;

const ELIGIBLE: &str = "JOIN recordings recording ON recording.id=wanted.recording_id
    JOIN library_members member ON member.library_id=recording.library_id AND member.user_id=wanted.account_id
    WHERE wanted.next_attempt_at<=unixepoch()
        AND coalesce((SELECT keep_played_tracks FROM account_automatic_copies WHERE user_id=wanted.account_id),1)=1
        AND NOT EXISTS(SELECT 1 FROM sources local WHERE local.recording_id=wanted.recording_id AND local.provider='local' AND local.ready=1)
        AND NOT EXISTS(SELECT 1 FROM podcast_episodes episode WHERE episode.recording_id=wanted.recording_id)
        AND NOT EXISTS(SELECT 1 FROM download_intents intent WHERE intent.account_id=wanted.account_id
            AND intent.recording_id=wanted.recording_id AND intent.host_device=?1 AND intent.state!='removed'
            AND (intent.state!='failed' OR intent.updated_at>unixepoch()-?2))";
const LIVE_SESSION: &str = "JOIN sessions session ON session.token_hash=wanted.session_hash
    AND session.user_id=wanted.account_id AND session.expires_at>unixepoch()";

#[derive(Clone, Copy)]
enum Wanted {
    Upcoming,
    Playing,
    Played,
}

impl Wanted {
    fn label(self) -> &'static str {
        match self {
            Self::Upcoming => "upcoming",
            Self::Playing => "playing",
            Self::Played => "played",
        }
    }

    fn query(self) -> String {
        match self {
            Self::Upcoming => format!(
                "SELECT wanted.session_hash,wanted.client,wanted.account_id,wanted.recording_id
                 FROM upcoming_recordings wanted INDEXED BY upcoming_recordings_due {LIVE_SESSION} {ELIGIBLE}
                    AND wanted.announced_at>unixepoch()-?3
                 ORDER BY wanted.announced_at DESC,wanted.position,wanted.recording_id LIMIT 1"
            ),
            Self::Playing => format!(
                "SELECT wanted.session_hash,wanted.client,wanted.account_id,wanted.recording_id
                 FROM playing_recordings wanted {LIVE_SESSION} {ELIGIBLE}
                    AND wanted.announced_at>unixepoch()-?3
                 ORDER BY wanted.announced_at DESC,wanted.recording_id LIMIT 1"
            ),
            Self::Played => format!(
                "SELECT '' AS session_hash,'' AS client,wanted.account_id,wanted.recording_id
                 FROM played_copy_requests wanted INDEXED BY played_copy_requests_recent {ELIGIBLE}
                    AND wanted.played_at>unixepoch()-?3
                 ORDER BY wanted.played_at DESC,wanted.recording_id LIMIT 1"
            ),
        }
    }

    fn retry(self) -> &'static str {
        match self {
            Self::Upcoming => {
                "UPDATE upcoming_recordings SET next_attempt_at=unixepoch()+?1
                 WHERE session_hash=?2 AND client=?3 AND recording_id=?4 AND account_id=?5"
            }
            Self::Playing => {
                "UPDATE playing_recordings SET next_attempt_at=unixepoch()+?1
                 WHERE session_hash=?2 AND client=?3 AND recording_id=?4 AND account_id=?5"
            }
            Self::Played => {
                "UPDATE played_copy_requests SET next_attempt_at=unixepoch()+?1
                 WHERE recording_id=?4 AND account_id=?5"
            }
        }
    }
}

#[derive(sqlx::FromRow)]
struct Candidate {
    session_hash: String,
    client: String,
    account_id: String,
    recording_id: String,
}

enum Prepared {
    Queued,
    Deferred(&'static str, i64),
}

impl Downloads {
    #[tracing::instrument(level = "debug", skip_all, err(Debug))]
    pub async fn run_next_copy_until(
        &self,
        shutdown: impl Future<Output = ()> + Send,
    ) -> Result<bool> {
        let (stop, stopped) = watch::channel(false);
        let (done, finished) = tokio::sync::oneshot::channel::<()>();
        let work = async move {
            let result = self.run_copy(stopped).await;
            drop(done);
            result
        };
        let cancellation = async move {
            tokio::select! { _ = shutdown => { stop.send_replace(true); }, _ = finished => {} }
        };
        let (result, ()) = tokio::join!(work, cancellation);
        result
    }

    async fn run_copy(&self, stopped: watch::Receiver<bool>) -> Result<bool> {
        if *stopped.borrow() {
            return Ok(false);
        }
        let saving_allowed = !self.disk_low();
        if saving_allowed && self.run_job(stopped.clone(), Claim::Automatic).await? {
            return Ok(true);
        }
        let Some((wanted, candidate)) = self.next_candidate().await? else {
            return Ok(false);
        };
        if !saving_allowed {
            tracing::debug!(
                event = "automatic_copy_deferred",
                outcome = "low_disk",
                "Automatic copies wait for free disk space"
            );
            return Ok(false);
        }
        let started = Instant::now();
        let prepared = self.prepare(&candidate, stopped).await?;
        let outcome = match prepared {
            Prepared::Queued => {
                if matches!(wanted, Wanted::Played) {
                    sqlx::query(
                        "DELETE FROM played_copy_requests WHERE account_id=? AND recording_id=?",
                    )
                    .bind(&candidate.account_id)
                    .bind(&candidate.recording_id)
                    .execute(&self.store.pool)
                    .await?;
                }
                "queued"
            }
            Prepared::Deferred(outcome, seconds) => {
                sqlx::query(wanted.retry())
                    .bind(seconds)
                    .bind(&candidate.session_hash)
                    .bind(&candidate.client)
                    .bind(&candidate.recording_id)
                    .bind(&candidate.account_id)
                    .execute(&self.store.pool)
                    .await?;
                outcome
            }
        };
        tracing::info!(
            event = "automatic_copy_prepared",
            account_id = %candidate.account_id,
            wanted = wanted.label(),
            outcome,
            elapsed_ms = started.elapsed().as_millis() as u64,
            "Upcoming recording considered for an automatic copy"
        );
        Ok(true)
    }

    pub(super) fn disk_low(&self) -> bool {
        match self.files.disk_space() {
            Ok(space) => space.below_percent(LOW_DISK_PERCENT),
            Err(error) => {
                tracing::debug!(
                    error_code = error.code(),
                    "Free disk space could not be read"
                );
                true
            }
        }
    }

    pub(super) async fn enforce_budget(&self, account: &str) {
        let evicted = async {
            let mut tx = self.store.pool.begin_with("BEGIN IMMEDIATE").await?;
            let retired = self.evict_on(&mut tx, account, Room::WithinBudget).await?;
            tx.commit().await?;
            Ok::<_, Error>(retired)
        }
        .await;
        match evicted {
            Ok(retired) => {
                let evicted = retired.0.len();
                self.reclaim_retired(retired).await;
                if evicted > 0 {
                    tracing::info!(
                        event = "automatic_copies_evicted",
                        account_id = account,
                        evicted,
                        "Automatic copies removed to stay within the budget"
                    );
                }
            }
            Err(error) => tracing::warn!(
                event = "automatic_copy_eviction_failed",
                error_code = error.code(),
                "Automatic copy eviction failed"
            ),
        }
    }

    async fn next_candidate(&self) -> Result<Option<(Wanted, Candidate)>> {
        for wanted in [Wanted::Upcoming, Wanted::Playing, Wanted::Played] {
            let candidate = sqlx::query_as(&wanted.query())
                .bind(self.host.as_str())
                .bind(FAILED_RETRY_SECONDS)
                .bind(UPCOMING_SECONDS)
                .fetch_optional(&self.store.pool)
                .await?;
            if let Some(candidate) = candidate {
                return Ok(Some((wanted, candidate)));
            }
        }
        Ok(None)
    }

    async fn prepare(
        &self,
        candidate: &Candidate,
        mut stopped: watch::Receiver<bool>,
    ) -> Result<Prepared> {
        let account = candidate.account_id.as_str();
        let recording = candidate.recording_id.as_str();
        let mut tx = self.store.pool.begin_with("BEGIN IMMEDIATE").await?;
        let retired = self.evict_on(&mut tx, account, Room::ForAnother).await?;
        let (_, budget) = preference_on(&mut tx, account).await?;
        let room = self.used_bytes_on(&mut tx, account).await? < budget.max(0) as u64;
        tx.commit().await?;
        self.reclaim_retired(retired).await;
        if !room {
            return Ok(Prepared::Deferred("over_budget", OVER_BUDGET_RETRY_SECONDS));
        }
        if let Some(prepared) = self.admit_copy(account, recording).await? {
            return Ok(prepared);
        }
        let actor = {
            let mut connection = self.store.pool.acquire().await?;
            match Actor::unattended_on(&mut connection, account).await {
                Ok(actor) => actor,
                Err(Error::NotFound) => {
                    return Ok(Prepared::Deferred("account_missing", UPCOMING_SECONDS));
                }
                Err(error) => return Err(error),
            }
        };
        let resolved = self
            .catalog
            .resolve_confident_source_in_background(&actor, recording, async move {
                let _ = stopped.wait_for(|stopped| *stopped).await;
            })
            .await;
        match resolved {
            Ok(Some(_)) => {}
            Ok(None) => return Ok(Prepared::Deferred("resolution_busy", BUSY_RETRY_SECONDS)),
            Err(error) => {
                tracing::debug!(
                    error_code = error.code(),
                    "Upcoming recording source resolution did not finish"
                );
                return Ok(Prepared::Deferred("no_source", NO_SOURCE_RETRY_SECONDS));
            }
        }
        Ok(self
            .admit_copy(account, recording)
            .await?
            .unwrap_or(Prepared::Deferred("no_source", NO_SOURCE_RETRY_SECONDS)))
    }

    async fn admit_copy(&self, account: &str, recording: &str) -> Result<Option<Prepared>> {
        let mut tx = self.store.pool.begin_with("BEGIN IMMEDIATE").await?;
        let access: Option<(String, String)> = sqlx::query_as(
            "SELECT r.library_id,g.id FROM recordings r
             JOIN library_access_grants g ON g.library_id=r.library_id AND g.user_id=? WHERE r.id=?",
        )
        .bind(account)
        .bind(recording)
        .fetch_optional(&mut *tx)
        .await?;
        let Some((library, grant)) = access else {
            return Ok(Some(Prepared::Deferred("no_access", UPCOMING_SECONDS)));
        };
        match self
            .plugins
            .capture_download_origin_on(&mut tx, account, recording)
            .await?
        {
            OriginSelection::Available(_) => {}
            OriginSelection::Restricted => {
                return Ok(Some(Prepared::Deferred("playback_only", UPCOMING_SECONDS)));
            }
            OriginSelection::Unavailable => return Ok(None),
        }
        let prepared = match self
            .admit_transfer_on(&mut tx, account, recording, &library, &grant)
            .await?
        {
            Admission::Retained(_) => {
                retain_copy_on(&mut tx, account, self.host.as_str(), recording, &grant).await?;
                Prepared::Queued
            }
            Admission::Busy => Prepared::Deferred("capacity", BUSY_RETRY_SECONDS),
            Admission::WaitingForSource => return Ok(None),
            Admission::Restricted => Prepared::Deferred("playback_only", UPCOMING_SECONDS),
        };
        tx.commit().await?;
        Ok(Some(prepared))
    }
}

pub(crate) async fn request_copy_after_play_on(
    connection: &mut SqliteConnection,
    account: &str,
    recording: &str,
) -> Result<bool> {
    sqlx::query("DELETE FROM played_copy_requests WHERE played_at<=unixepoch()-?")
        .bind(UPCOMING_SECONDS)
        .execute(&mut *connection)
        .await?;
    let requested = sqlx::query(
        "INSERT INTO played_copy_requests(account_id,recording_id) SELECT ?1,?2
         WHERE coalesce((SELECT keep_played_tracks FROM account_automatic_copies WHERE user_id=?1),1)=1
            AND NOT EXISTS(SELECT 1 FROM sources local WHERE local.recording_id=?2 AND local.provider='local' AND local.ready=1)
            AND NOT EXISTS(SELECT 1 FROM download_intents intent WHERE intent.account_id=?1
                AND intent.recording_id=?2 AND intent.state='ready')
         ON CONFLICT(account_id,recording_id) DO UPDATE SET played_at=unixepoch()",
    )
    .bind(account)
    .bind(recording)
    .execute(connection)
    .await?
    .rows_affected();
    Ok(requested > 0)
}

async fn retain_copy_on(
    connection: &mut SqliteConnection,
    account: &str,
    host: &str,
    recording: &str,
    grant: &str,
) -> Result<()> {
    sqlx::query(
        "INSERT INTO automatic_copies(account_id,host_device,recording_id,access_grant_id) VALUES(?,?,?,?)
         ON CONFLICT(account_id,recording_id,host_device) DO UPDATE SET access_grant_id=excluded.access_grant_id",
    )
    .bind(account)
    .bind(host)
    .bind(recording)
    .bind(grant)
    .execute(connection)
    .await?;
    Ok(())
}
