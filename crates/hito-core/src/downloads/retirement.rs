use super::{Downloads, lease::stored_uuid};
use crate::Result;
use sqlx::SqliteConnection;

impl Downloads {
    pub(super) async fn retain_retirement_candidate_on(
        &self,
        connection: &mut SqliteConnection,
        transfer: &str,
    ) -> Result<()> {
        let direct: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM download_intents i JOIN download_retentions root
            ON root.recording_id=i.recording_id AND root.account_id=i.account_id AND root.host_device=i.host_device
                AND root.access_grant_id=i.access_grant_id AND root.kind='recording' AND root.state='active' WHERE i.id=?)")
            .bind(transfer).fetch_one(&mut *connection).await?;
        if direct {
            sqlx::query("DELETE FROM download_retirement_work WHERE transfer_id=?")
                .bind(transfer)
                .execute(connection)
                .await?;
            return Ok(());
        }
        let playlist: Option<String> = sqlx::query_scalar("SELECT p.id FROM download_intents i
            JOIN playlist_entries e ON e.recording_id=i.recording_id AND e.deleted=0
            JOIN playlists p ON p.id=e.playlist_id AND p.user_id=i.account_id AND p.deleted=0
            JOIN download_retentions root ON root.playlist_id=p.id AND root.account_id=i.account_id AND root.host_device=i.host_device
                AND root.kind='playlist' AND root.state='active' WHERE i.id=? ORDER BY p.id LIMIT 1")
            .bind(transfer).fetch_optional(&mut *connection).await?;
        if let Some(playlist) = playlist {
            sqlx::query("INSERT INTO download_retirement_work(transfer_id,host_device,playlist_id,next_check_at) VALUES(?,?,?,unixepoch()+5)
                ON CONFLICT(transfer_id) DO UPDATE SET playlist_id=excluded.playlist_id,next_check_at=excluded.next_check_at")
                .bind(transfer).bind(self.host.as_str()).bind(playlist).execute(connection).await?;
        }
        Ok(())
    }

    pub(super) async fn reconsider_retirement(&self) -> Result<bool> {
        let eligible: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM download_retirement_work WHERE host_device=? AND next_check_at<=unixepoch())")
            .bind(self.host.as_str()).fetch_one(&self.store.pool).await?;
        if !eligible {
            return Ok(false);
        }
        let mut tx = self.store.pool.begin_with("BEGIN IMMEDIATE").await?;
        let candidates: Vec<(String, String, String, String)> = sqlx::query_as("SELECT work.transfer_id,i.account_id,i.recording_id,i.state
            FROM download_retirement_work work JOIN download_intents i ON i.id=work.transfer_id
            WHERE work.host_device=? AND work.next_check_at<=unixepoch() ORDER BY work.next_check_at,work.transfer_id LIMIT 32")
            .bind(self.host.as_str()).fetch_all(&mut *tx).await?;
        let mut files = Vec::new();
        for (transfer, account, recording, state) in &candidates {
            if state == "removed" {
                sqlx::query("DELETE FROM download_retirement_work WHERE transfer_id=?")
                    .bind(transfer)
                    .execute(&mut *tx)
                    .await?;
            } else if let Some(file) = self
                .retire_unreferenced_on(&mut tx, account, recording)
                .await?
            {
                files.push(file);
            }
        }
        tx.commit().await?;
        for (file, attempt) in files {
            self.files
                .revoke(stored_uuid(&file)?, stored_uuid(&attempt)?)?;
        }
        tracing::debug!(
            candidate_count = candidates.len(),
            "Shared download retirement candidates checked"
        );
        Ok(!candidates.is_empty())
    }
}
