use super::{
    Downloads,
    copies::{Retired, preference_on},
};
use crate::Result;
use sqlx::SqliteConnection;

pub(super) const UPCOMING_SECONDS: i64 = 6 * 60 * 60;
const PLAYING_SECONDS: i64 = 15 * 60;
const PLAY_WEIGHT_SECONDS: i64 = 7 * 24 * 60 * 60;
const COUNTED_PLAYS: i64 = 8;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Room {
    WithinBudget,
    ForAnother,
}

impl Downloads {
    pub(super) async fn evict_on(
        &self,
        connection: &mut SqliteConnection,
        account: &str,
        room: Room,
    ) -> Result<Retired> {
        let (_, budget) = preference_on(connection, account).await?;
        let budget = budget.max(0) as u64;
        let mut used = self.used_bytes_on(connection, account).await?;
        let over = |used: u64| match room {
            Room::WithinBudget => used > budget,
            Room::ForAnother => used >= budget,
        };
        if !over(used) {
            return Ok(Retired(Vec::new()));
        }
        let candidates: Vec<(String, i64)> = sqlx::query_as(
            "SELECT copy.recording_id,file.byte_length FROM automatic_copies copy
             JOIN download_intents intent ON intent.account_id=copy.account_id AND intent.recording_id=copy.recording_id
                AND intent.host_device=copy.host_device AND intent.access_grant_id=copy.access_grant_id AND intent.state='ready'
             JOIN download_files file ON file.id=intent.file_id AND file.state='ready'
             LEFT JOIN listening_history history ON history.user_id=copy.account_id AND history.recording_id=copy.recording_id
             WHERE copy.account_id=?1 AND copy.host_device=?2
                AND NOT EXISTS(SELECT 1 FROM chosen_download_intents chosen WHERE chosen.id=intent.id)
                AND NOT EXISTS(SELECT 1 FROM upcoming_recordings upcoming WHERE upcoming.account_id=copy.account_id
                    AND upcoming.recording_id=copy.recording_id AND upcoming.announced_at>unixepoch()-?3)
                AND NOT EXISTS(SELECT 1 FROM listening_progress progress WHERE progress.user_id=copy.account_id
                    AND progress.recording_id=copy.recording_id AND progress.updated_at>unixepoch()-?4)
             ORDER BY coalesce(history.played_at,0)+?5*min(coalesce(history.play_count,0),?6),copy.created_at,copy.recording_id",
        )
        .bind(account)
        .bind(self.host.as_str())
        .bind(UPCOMING_SECONDS)
        .bind(PLAYING_SECONDS)
        .bind(PLAY_WEIGHT_SECONDS)
        .bind(COUNTED_PLAYS)
        .fetch_all(&mut *connection)
        .await?;
        let mut retired = Vec::new();
        for (recording, bytes) in candidates {
            if !over(used) {
                break;
            }
            sqlx::query(
                "DELETE FROM automatic_copies WHERE account_id=? AND recording_id=? AND host_device=?",
            )
            .bind(account)
            .bind(&recording)
            .bind(self.host.as_str())
            .execute(&mut *connection)
            .await?;
            if let Some(file) = self.retire_copy_on(connection, account, &recording).await? {
                retired.push(file);
            }
            used = used.saturating_sub(bytes.max(0) as u64);
        }
        tracing::debug!(
            event = "automatic_copies_evicted",
            evicted = retired.len(),
            within_budget = !over(used),
            "Automatic copies evicted by score"
        );
        Ok(Retired(retired))
    }
}
