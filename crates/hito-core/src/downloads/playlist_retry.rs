use super::{Downloads, retention::Admission};
use crate::Result;
use std::time::Instant;

#[derive(sqlx::FromRow)]
struct RetryWork {
    retention_id: String,
    request_id: String,
    account_id: String,
    playlist_id: String,
    playlist_revision: i64,
    failure_cutoff: i64,
    cursor: Option<String>,
    live: bool,
}

impl Downloads {
    #[tracing::instrument(level = "debug", skip_all, err(Debug))]
    pub(super) async fn retry_playlist_transfers(&self) -> Result<bool> {
        let eligible: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM download_playlist_retry_work
            WHERE host_device=? AND next_attempt_at<=unixepoch())",
        )
        .bind(self.host.as_str())
        .fetch_one(&self.store.pool)
        .await?;
        if !eligible {
            return Ok(false);
        }
        let started = Instant::now();
        let mut tx = self.store.pool.begin_with("BEGIN IMMEDIATE").await?;
        let selected: Option<RetryWork> = sqlx::query_as("SELECT work.retention_id,work.request_id,root.account_id,root.playlist_id,
            work.playlist_revision,work.failure_cutoff,work.cursor,
            (root.state='active' AND root.kind='playlist' AND p.deleted=0 AND p.user_id=root.account_id) AS live
            FROM download_playlist_retry_work work JOIN download_retentions root
                ON root.id=work.retention_id AND root.host_device=work.host_device
            JOIN playlists p ON p.id=root.playlist_id
            WHERE work.host_device=? AND work.next_attempt_at<=unixepoch()
            ORDER BY work.next_attempt_at,work.retention_id LIMIT 1")
            .bind(self.host.as_str()).fetch_optional(&mut *tx).await?;
        let Some(selected) = selected else {
            tx.commit().await?;
            return Ok(false);
        };
        if !selected.live || selected.failure_cutoff == 0 {
            sqlx::query("DELETE FROM download_playlist_retry_work WHERE retention_id=?")
                .bind(&selected.retention_id)
                .execute(&mut *tx)
                .await?;
            tx.commit().await?;
            tracing::debug!(retention_id = %selected.retention_id, request_id = %selected.request_id,
                outcome = if selected.live { "no_failures" } else { "withdrawn" }, "Playlist download retry finished");
            return Ok(true);
        }
        let mut recordings: Vec<String> = sqlx::query_scalar("SELECT recording_id FROM playlist_entries INDEXED BY playlist_download_retry_scan
            WHERE playlist_id=? AND deleted=0 AND recording_id>coalesce(?,'') ORDER BY recording_id LIMIT 32")
            .bind(&selected.playlist_id).bind(&selected.cursor).fetch_all(&mut *tx).await?;
        let entry_count = recordings.len();
        recordings.dedup();
        let mut cursor = selected.cursor.clone();
        let mut deferred = false;
        for recording in &recordings {
            let authority: Option<(String, String)> = sqlx::query_as("SELECT recording.library_id,access.id
                FROM download_intents intent JOIN download_terminal_failures failure ON failure.intent_id=intent.id
                JOIN recordings recording ON recording.id=intent.recording_id AND recording.library_id=intent.library_id
                JOIN library_access_grants access ON access.library_id=recording.library_id AND access.user_id=intent.account_id
                WHERE intent.account_id=? AND intent.recording_id=? AND intent.host_device=?
                    AND intent.state!='removed' AND intent.state='failed' AND failure.sequence<=?")
                .bind(&selected.account_id).bind(recording).bind(self.host.as_str()).bind(selected.failure_cutoff)
                .fetch_optional(&mut *tx).await?;
            let Some((library, grant)) = authority else {
                cursor = Some(recording.clone());
                continue;
            };
            sqlx::query("SAVEPOINT playlist_download_retry_admission")
                .execute(&mut *tx)
                .await?;
            let admission = self
                .admit_transfer_on(&mut tx, &selected.account_id, recording, &library, &grant)
                .await?;
            let (rollback, busy, outcome) = match admission {
                Admission::Retained(transfer) => {
                    tracing::debug!(retention_id = %selected.retention_id, request_id = %selected.request_id,
                        transfer_id = %transfer, "Playlist download retry transfer retained");
                    (false, false, "retained")
                }
                Admission::Busy => (true, true, "capacity_deferred"),
                Admission::WaitingForSource => (true, false, "source_unavailable"),
            };
            if rollback {
                sqlx::query("ROLLBACK TO SAVEPOINT playlist_download_retry_admission")
                    .execute(&mut *tx)
                    .await?;
            }
            sqlx::query("RELEASE SAVEPOINT playlist_download_retry_admission")
                .execute(&mut *tx)
                .await?;
            tracing::debug!(retention_id = %selected.retention_id, request_id = %selected.request_id,
                recording_id = %recording, outcome, "Playlist download retry admission checked");
            if busy {
                deferred = true;
                break;
            }
            cursor = Some(recording.clone());
        }
        let complete = !deferred && entry_count < 32;
        if complete {
            sqlx::query("DELETE FROM download_playlist_retry_work WHERE retention_id=?")
                .bind(&selected.retention_id)
                .execute(&mut *tx)
                .await?;
        } else {
            sqlx::query("UPDATE download_playlist_retry_work SET cursor=?,next_attempt_at=unixepoch()+? WHERE retention_id=?")
                .bind(cursor).bind(if deferred { 5_i64 } else { 1 }).bind(&selected.retention_id)
                .execute(&mut *tx).await?;
        }
        tx.commit().await?;
        tracing::debug!(retention_id = %selected.retention_id, request_id = %selected.request_id,
            revision = selected.playlist_revision, failure_cutoff = selected.failure_cutoff,
            entry_count, recording_count = recordings.len(), deferred, complete,
            elapsed_ms = started.elapsed().as_millis() as u64, "Playlist download retry batch finished");
        Ok(true)
    }
}
