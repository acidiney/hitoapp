use super::{Admission, Downloads, RetryWork};
use crate::{Result, downloads::OriginSelection};
use sqlx::SqliteConnection;

pub(super) async fn defer_on(
    connection: &mut SqliteConnection,
    work: &RetryWork,
    recording: &str,
) -> Result<()> {
    sqlx::query("INSERT INTO download_playlist_retry_waiting(retention_id,host_device,recording_id,failed_intent_id,command_generation,next_attempt_at)
        SELECT ?,?,intent.recording_id,intent.id,?,unixepoch()+30
        FROM download_intents intent JOIN download_terminal_failures failure ON failure.intent_id=intent.id
        WHERE intent.account_id=? AND intent.recording_id=? AND intent.host_device=?
            AND intent.state!='removed' AND intent.state='failed' AND failure.sequence<=?
        ON CONFLICT(retention_id,recording_id) DO UPDATE SET failed_intent_id=excluded.failed_intent_id,
            command_generation=excluded.command_generation,next_attempt_at=excluded.next_attempt_at")
        .bind(&work.retention_id).bind(&work.host_device).bind(work.command_generation)
        .bind(&work.account_id).bind(recording).bind(&work.host_device).bind(work.failure_cutoff)
        .execute(connection).await?;
    Ok(())
}

impl Downloads {
    pub(super) async fn retry_waiting_on(
        &self,
        connection: &mut SqliteConnection,
        work: &RetryWork,
    ) -> Result<usize> {
        let waiting: Vec<(String, String)> = sqlx::query_as("SELECT recording_id,failed_intent_id
            FROM download_playlist_retry_waiting INDEXED BY download_playlist_retry_waiting_due
            WHERE retention_id=? AND host_device=? AND command_generation=? AND next_attempt_at<=unixepoch()
            ORDER BY next_attempt_at,recording_id LIMIT 32")
            .bind(&work.retention_id).bind(&work.host_device).bind(work.command_generation)
            .fetch_all(&mut *connection).await?;
        for (recording, failed_intent) in &waiting {
            let authority: Option<(String, String)> = sqlx::query_as("SELECT recording.library_id,access.id
                FROM download_intents intent JOIN download_terminal_failures failure ON failure.intent_id=intent.id
                JOIN recordings recording ON recording.id=intent.recording_id AND recording.library_id=intent.library_id
                JOIN library_access_grants access ON access.library_id=recording.library_id AND access.user_id=intent.account_id
                WHERE intent.id=? AND intent.account_id=? AND intent.recording_id=? AND intent.host_device=?
                    AND intent.state='failed' AND failure.sequence<=?
                    AND EXISTS(SELECT 1 FROM playlist_entries INDEXED BY playlist_download_retry_scan
                        WHERE playlist_id=? AND deleted=0 AND recording_id=intent.recording_id)")
                .bind(failed_intent).bind(&work.account_id).bind(recording).bind(&work.host_device)
                .bind(work.failure_cutoff).bind(&work.playlist_id).fetch_optional(&mut *connection).await?;
            let (delay, outcome) = if let Some((library, grant)) = authority {
                let admission = match self
                    .capture_origin_on(connection, &work.account_id, recording)
                    .await?
                {
                    OriginSelection::Available(_) => {
                        self.retry_admission_on(connection, work, recording, &library, &grant)
                            .await?
                    }
                    OriginSelection::Unavailable => Admission::WaitingForSource,
                    OriginSelection::Restricted => Admission::Restricted,
                };
                match admission {
                    Admission::Retained(_) => (None, "retained"),
                    Admission::Busy => (Some(5_i64), "capacity_deferred"),
                    Admission::WaitingForSource => (Some(30), "source_deferred"),
                    Admission::Restricted => (None, "policy_restricted"),
                }
            } else {
                (None, "withdrawn")
            };
            if let Some(delay) = delay {
                sqlx::query(
                    "UPDATE download_playlist_retry_waiting SET next_attempt_at=unixepoch()+?
                    WHERE retention_id=? AND recording_id=?",
                )
                .bind(delay)
                .bind(&work.retention_id)
                .bind(recording)
                .execute(&mut *connection)
                .await?;
            } else {
                sqlx::query("DELETE FROM download_playlist_retry_waiting WHERE retention_id=? AND recording_id=?")
                    .bind(&work.retention_id).bind(recording).execute(&mut *connection).await?;
            }
            tracing::debug!(retention_id = %work.retention_id, recording_id = %recording, outcome,
                "Deferred playlist download retry checked");
        }
        Ok(waiting.len())
    }
}

pub(super) async fn prune_on(connection: &mut SqliteConnection, work: &RetryWork) -> Result<()> {
    let recordings: Vec<String> = if work.live && work.failure_cutoff > 0 {
        sqlx::query_scalar("SELECT recording_id FROM download_playlist_retry_waiting
            INDEXED BY download_playlist_retry_waiting_obsolete WHERE retention_id=? AND command_generation<?
            ORDER BY command_generation,recording_id LIMIT 32")
            .bind(&work.retention_id).bind(work.command_generation).fetch_all(&mut *connection).await?
    } else {
        sqlx::query_scalar(
            "SELECT recording_id FROM download_playlist_retry_waiting
            WHERE retention_id=? ORDER BY recording_id LIMIT 32",
        )
        .bind(&work.retention_id)
        .fetch_all(&mut *connection)
        .await?
    };
    for recording in &recordings {
        sqlx::query(
            "DELETE FROM download_playlist_retry_waiting WHERE retention_id=? AND recording_id=?",
        )
        .bind(&work.retention_id)
        .bind(recording)
        .execute(&mut *connection)
        .await?;
    }
    tracing::debug!(retention_id = %work.retention_id, item_count = recordings.len(),
        "Obsolete playlist download retries pruned");
    Ok(())
}

pub(super) async fn schedule_on(connection: &mut SqliteConnection, work: &RetryWork) -> Result<()> {
    let now = work.now;
    let live = work.live && work.failure_cutoff > 0;
    let next = if live {
        let waiting: Option<i64> = sqlx::query_scalar("SELECT next_attempt_at FROM download_playlist_retry_waiting
            INDEXED BY download_playlist_retry_waiting_due WHERE retention_id=? AND host_device=? AND command_generation=?
            ORDER BY next_attempt_at,recording_id LIMIT 1")
            .bind(&work.retention_id).bind(&work.host_device).bind(work.command_generation)
            .fetch_optional(&mut *connection).await?;
        let obsolete: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM download_playlist_retry_waiting
            INDEXED BY download_playlist_retry_waiting_obsolete WHERE retention_id=? AND command_generation<?)")
            .bind(&work.retention_id).bind(work.command_generation).fetch_one(&mut *connection).await?;
        [
            (!work.scan_complete).then_some(work.scan_next_attempt_at.max(now + 1)),
            waiting.map(|due| due.max(now + 1)),
            obsolete.then_some(now + 1),
        ]
        .into_iter()
        .flatten()
        .min()
    } else {
        let pending: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM download_playlist_retry_waiting WHERE retention_id=?)",
        )
        .bind(&work.retention_id)
        .fetch_one(&mut *connection)
        .await?;
        pending.then_some(now + 1)
    };
    if let Some(next) = next {
        sqlx::query("UPDATE download_playlist_retry_work SET cursor=?,scan_complete=?,scan_next_attempt_at=?,next_attempt_at=? WHERE retention_id=?")
            .bind(&work.cursor).bind(work.scan_complete || !live).bind(work.scan_next_attempt_at).bind(next).bind(&work.retention_id).execute(connection).await?;
    } else {
        sqlx::query("DELETE FROM download_playlist_retry_work WHERE retention_id=?")
            .bind(&work.retention_id)
            .execute(connection)
            .await?;
    }
    tracing::debug!(retention_id = %work.retention_id, complete = next.is_none(), "Playlist download retry scheduled");
    Ok(())
}
