use super::{Downloads, retention::Admission};
use crate::Result;

#[derive(sqlx::FromRow)]
struct Candidate {
    retention_id: String,
    entry_id: String,
    recording_id: Option<String>,
    account_id: String,
    revision: i64,
    seen_revision: i64,
    live: bool,
    library_id: Option<String>,
    grant_id: Option<String>,
}

impl Downloads {
    #[tracing::instrument(level = "debug", skip_all, err(Debug))]
    pub(super) async fn admit_playlist_transfers(&self) -> Result<bool> {
        let eligible: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM download_playlist_members
            WHERE host_device=? AND next_attempt_at IS NOT NULL AND next_attempt_at<=unixepoch())",
        )
        .bind(self.host.as_str())
        .fetch_one(&self.store.pool)
        .await?;
        if !eligible {
            return Ok(false);
        }
        let mut tx = self.store.pool.begin_with("BEGIN IMMEDIATE").await?;
        let candidates: Vec<Candidate> = sqlx::query_as("SELECT member.retention_id,member.entry_id,member.recording_id,root.account_id,
            changes.revision,member.seen_revision,recording.library_id,access.id AS grant_id,
            (root.state='active' AND p.deleted=0 AND p.user_id=root.account_id AND entry.id IS NOT NULL
                AND entry.deleted=0 AND entry.recording_id IS member.recording_id) AS live
            FROM download_playlist_members member JOIN download_retentions root
                ON root.id=member.retention_id AND root.host_device=member.host_device
            JOIN playlists p ON p.id=root.playlist_id JOIN download_playlist_changes changes ON changes.playlist_id=p.id
            LEFT JOIN playlist_entries entry ON entry.playlist_id=p.id AND entry.id=member.entry_id
            LEFT JOIN recordings recording ON recording.id=member.recording_id
            LEFT JOIN library_access_grants access ON access.library_id=recording.library_id AND access.user_id=root.account_id
            WHERE member.host_device=? AND member.next_attempt_at IS NOT NULL AND member.next_attempt_at<=unixepoch()
            ORDER BY member.next_attempt_at,member.retention_id,member.entry_id LIMIT 32")
            .bind(self.host.as_str()).fetch_all(&mut *tx).await?;
        let mut deferred = 0;
        for candidate in &candidates {
            let (delay, outcome) = if !candidate.live {
                (None, "withdrawn")
            } else if candidate.revision != candidate.seen_revision {
                (Some(5_i64), "awaiting_revision")
            } else if let (Some(recording), Some(library), Some(grant)) = (
                &candidate.recording_id,
                &candidate.library_id,
                &candidate.grant_id,
            ) {
                let failed: bool = sqlx::query_scalar(
                    "SELECT EXISTS(SELECT 1 FROM download_intents
                    WHERE account_id=? AND recording_id=? AND host_device=? AND state='failed')",
                )
                .bind(&candidate.account_id)
                .bind(recording)
                .bind(self.host.as_str())
                .fetch_one(&mut *tx)
                .await?;
                if failed {
                    (None, "failed")
                } else {
                    sqlx::query("SAVEPOINT playlist_download_admission")
                        .execute(&mut *tx)
                        .await?;
                    let admission = self
                        .admit_transfer_on(
                            &mut tx,
                            &candidate.account_id,
                            recording,
                            library,
                            grant,
                        )
                        .await?;
                    let result = match admission {
                        Admission::Retained(transfer) => {
                            tracing::debug!(retention_id = %candidate.retention_id, entry_id = %candidate.entry_id,
                                transfer_id = %transfer, "Playlist download transfer retained");
                            (None, "retained")
                        }
                        Admission::Busy => (Some(5), "capacity_deferred"),
                        Admission::WaitingForSource => (Some(30), "source_deferred"),
                    };
                    if result.0.is_some() {
                        sqlx::query("ROLLBACK TO SAVEPOINT playlist_download_admission")
                            .execute(&mut *tx)
                            .await?;
                    }
                    sqlx::query("RELEASE SAVEPOINT playlist_download_admission")
                        .execute(&mut *tx)
                        .await?;
                    result
                }
            } else if candidate.recording_id.is_some() {
                (Some(30), "access_deferred")
            } else {
                (None, "unavailable")
            };
            deferred += usize::from(delay.is_some());
            sqlx::query(
                "UPDATE download_playlist_members SET next_attempt_at=unixepoch()+?
                WHERE retention_id=? AND entry_id=?",
            )
            .bind(delay)
            .bind(&candidate.retention_id)
            .bind(&candidate.entry_id)
            .execute(&mut *tx)
            .await?;
            tracing::debug!(retention_id = %candidate.retention_id, entry_id = %candidate.entry_id,
                outcome, retry_seconds = delay, "Playlist download admission checked");
        }
        tx.commit().await?;
        tracing::debug!(
            candidate_count = candidates.len(),
            deferred,
            "Playlist download admission batch finished"
        );
        Ok(!candidates.is_empty())
    }
}
