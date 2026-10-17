use super::{Downloads, lease::stored_uuid};
use crate::Result;
use std::{future::Future, sync::atomic::Ordering, time::Instant};
use tokio::sync::{oneshot, watch};

#[derive(sqlx::FromRow)]
struct Work {
    retention_id: String,
    account_id: String,
    playlist_id: String,
    revision: i64,
    scan_revision: i64,
    phase: String,
    cursor: Option<String>,
    live: bool,
}

impl Downloads {
    #[tracing::instrument(level = "debug", skip_all, err(Debug))]
    pub async fn run_next_playlist_reconciliation_until(
        &self,
        shutdown: impl Future<Output = ()> + Send,
    ) -> Result<bool> {
        let started = Instant::now();
        let (stop, stopped) = watch::channel(false);
        let (done, finished) = oneshot::channel::<()>();
        let work = async {
            let _completion = done;
            let first = self.playlist_turn.fetch_add(1, Ordering::Relaxed) % 4;
            for offset in 0..4 {
                if *stopped.borrow() {
                    return Ok(false);
                }
                let progressed = match (first + offset) % 4 {
                    0 => self.dispatch_playlist_change().await?,
                    1 => self.reconcile_playlist().await?,
                    2 => self.admit_playlist_transfers().await?,
                    _ => self.reconsider_retirement().await?,
                };
                if progressed {
                    return Ok(true);
                }
            }
            Ok(false)
        };
        let cancellation = async {
            tokio::select! { _ = shutdown => { stop.send_replace(true); }, _ = finished => {} }
        };
        let (result, ()) = tokio::join!(work, cancellation);
        tracing::debug!(
            progressed = result.as_ref().is_ok_and(|value| *value),
            elapsed_ms = started.elapsed().as_millis() as u64,
            "Playlist download reconciliation checked"
        );
        result
    }

    async fn reconcile_playlist(&self) -> Result<bool> {
        let eligible: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM download_playlist_work WHERE host_device=? AND status='queued' AND next_attempt_at<=unixepoch())")
            .bind(self.host.as_str()).fetch_one(&self.store.pool).await?;
        if !eligible {
            return Ok(false);
        }
        let mut tx = self.store.pool.begin_with("BEGIN IMMEDIATE").await?;
        let selected: Option<Work> = sqlx::query_as("SELECT work.retention_id,root.account_id,root.playlist_id,changes.revision,work.scan_revision,work.phase,work.cursor,
            (root.state='active' AND p.deleted=0 AND p.user_id=root.account_id) AS live
            FROM download_playlist_work work JOIN download_retentions root ON root.id=work.retention_id AND root.host_device=work.host_device
            JOIN playlists p ON p.id=root.playlist_id JOIN download_playlist_changes changes ON changes.playlist_id=p.id
            WHERE work.host_device=? AND work.status='queued' AND work.next_attempt_at<=unixepoch() ORDER BY work.next_attempt_at,work.retention_id LIMIT 1")
            .bind(self.host.as_str()).fetch_optional(&mut *tx).await?;
        let Some(mut selected) = selected else {
            tx.commit().await?;
            return Ok(false);
        };
        if !selected.live {
            sqlx::query("UPDATE download_retentions SET state='removed',updated_at=unixepoch() WHERE id=? AND state='active'")
                .bind(&selected.retention_id).execute(&mut *tx).await?;
            selected.phase = "prune".into();
        } else if selected.scan_revision != selected.revision || selected.phase == "idle" {
            selected.phase = "scan".into();
            selected.cursor = None;
        }
        let mut retired = Vec::new();
        let (count, complete) = if selected.phase == "scan" {
            let entries: Vec<(String, Option<String>)> = sqlx::query_as(
                "SELECT id,recording_id FROM playlist_entries
                WHERE playlist_id=? AND deleted=0 AND id>coalesce(?,'') ORDER BY id LIMIT 32",
            )
            .bind(&selected.playlist_id)
            .bind(&selected.cursor)
            .fetch_all(&mut *tx)
            .await?;
            for (entry, recording) in &entries {
                let previous_recording: Option<String> = sqlx::query_scalar(
                    "SELECT recording_id FROM download_playlist_members WHERE retention_id=? AND entry_id=?
                    AND recording_id IS NOT NULL AND recording_id IS NOT ?",
                )
                .bind(&selected.retention_id)
                .bind(entry)
                .bind(recording)
                .fetch_optional(&mut *tx)
                .await?;
                sqlx::query("INSERT INTO download_playlist_members(retention_id,host_device,entry_id,recording_id,seen_revision,next_attempt_at)
                    VALUES(?,?,?,?,?,CASE WHEN ? IS NOT NULL THEN 0 END)
                    ON CONFLICT(retention_id,entry_id) DO UPDATE SET recording_id=excluded.recording_id,seen_revision=excluded.seen_revision,
                    next_attempt_at=CASE WHEN recording_id IS excluded.recording_id THEN next_attempt_at ELSE excluded.next_attempt_at END")
                    .bind(&selected.retention_id).bind(self.host.as_str()).bind(entry).bind(recording).bind(selected.revision).bind(recording).execute(&mut *tx).await?;
                if let Some(previous_recording) = previous_recording {
                    if let Some(file) = self
                        .retire_unreferenced_on(&mut tx, &selected.account_id, &previous_recording)
                        .await?
                    {
                        retired.push(file);
                    }
                }
            }
            if entries.len() < 32 {
                selected.phase = "prune".into();
                selected.cursor = None;
            } else {
                selected.cursor = entries.last().map(|(id, _)| id.clone());
            }
            (entries.len(), false)
        } else {
            let mut query = sqlx::QueryBuilder::new(
                "SELECT entry_id,recording_id FROM download_playlist_members WHERE retention_id=",
            );
            query.push_bind(&selected.retention_id);
            if selected.live {
                query
                    .push(" AND seen_revision<")
                    .push_bind(selected.revision);
            }
            query.push(" ORDER BY seen_revision,entry_id LIMIT 32");
            let entries: Vec<(String, Option<String>)> =
                query.build_query_as().fetch_all(&mut *tx).await?;
            for (entry, recording) in &entries {
                sqlx::query(
                    "DELETE FROM download_playlist_members WHERE retention_id=? AND entry_id=?",
                )
                .bind(&selected.retention_id)
                .bind(entry)
                .execute(&mut *tx)
                .await?;
                if let Some(recording) = recording {
                    if let Some(file) = self
                        .retire_unreferenced_on(&mut tx, &selected.account_id, recording)
                        .await?
                    {
                        retired.push(file);
                    }
                }
            }
            (entries.len(), entries.len() < 32)
        };
        sqlx::query("UPDATE download_playlist_work SET desired_revision=?,scan_revision=?,reconciled_revision=CASE WHEN ? THEN ? ELSE reconciled_revision END,
            phase=?,cursor=?,status=?,next_attempt_at=unixepoch()+1 WHERE retention_id=?")
            .bind(selected.revision).bind(selected.revision).bind(complete).bind(selected.revision)
            .bind(if complete { "idle" } else { selected.phase.as_str() }).bind(selected.cursor)
            .bind(if complete { "idle" } else { "queued" }).bind(&selected.retention_id).execute(&mut *tx).await?;
        tx.commit().await?;
        for (file, attempt) in retired {
            self.files
                .revoke(stored_uuid(&file)?, stored_uuid(&attempt)?)?;
        }
        tracing::debug!(retention_id = %selected.retention_id, revision = selected.revision, item_count = count, complete, "Playlist download membership reconciled");
        Ok(true)
    }
}
