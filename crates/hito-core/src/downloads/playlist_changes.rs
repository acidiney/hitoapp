use super::Downloads;
use crate::Result;
use sqlx::SqliteConnection;
use uuid::Uuid;

#[tracing::instrument(level = "debug", skip_all, err(Debug), fields(playlist_id = %playlist))]
pub(crate) async fn playlist_changed_on(
    connection: &mut SqliteConnection,
    playlist: &Uuid,
) -> Result<()> {
    let changed = sqlx::query("UPDATE download_playlist_changes SET revision=revision+1,dirty=1,cursor=NULL,retirement_cursor=NULL,
        roots_done=0,retirements_done=0,next_dispatch_at=0 WHERE playlist_id=?
        AND EXISTS(SELECT 1 FROM download_retentions root WHERE root.playlist_id=download_playlist_changes.playlist_id AND root.kind='playlist' AND root.state='active')")
        .bind(playlist.to_string()).execute(connection).await?.rows_affected();
    tracing::debug!(changed, "Playlist download reconciliation marked");
    Ok(())
}

#[derive(sqlx::FromRow)]
struct Dispatch {
    playlist_id: String,
    revision: i64,
    cursor: Option<String>,
    retirement_cursor: Option<String>,
    roots_done: bool,
    retirements_done: bool,
}

impl Downloads {
    pub(super) async fn dispatch_playlist_change(&self) -> Result<bool> {
        let eligible: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM download_playlist_changes WHERE dirty=1 AND next_dispatch_at<=unixepoch())")
            .fetch_one(&self.store.pool).await?;
        if !eligible {
            return Ok(false);
        }
        let mut tx = self.store.pool.begin_with("BEGIN IMMEDIATE").await?;
        let selected: Option<Dispatch> = sqlx::query_as("SELECT playlist_id,revision,cursor,retirement_cursor,roots_done,retirements_done
            FROM download_playlist_changes WHERE dirty=1 AND next_dispatch_at<=unixepoch() ORDER BY next_dispatch_at,playlist_id LIMIT 1")
            .fetch_optional(&mut *tx).await?;
        let Some(mut selected) = selected else {
            tx.commit().await?;
            return Ok(false);
        };
        let mut count = 0;
        if !selected.roots_done {
            let roots: Vec<(String, String)> = sqlx::query_as("SELECT id,host_device FROM download_retentions
                WHERE playlist_id=? AND kind='playlist' AND state='active' AND id>coalesce(?,'') ORDER BY id LIMIT 16")
                .bind(&selected.playlist_id).bind(&selected.cursor).fetch_all(&mut *tx).await?;
            for (root, host) in &roots {
                sqlx::query("INSERT INTO download_playlist_work(retention_id,host_device,desired_revision) VALUES(?,?,?)
                    ON CONFLICT(retention_id) DO UPDATE SET desired_revision=max(desired_revision,excluded.desired_revision),
                    status=CASE WHEN reconciled_revision<excluded.desired_revision OR phase!='idle' THEN 'queued' ELSE status END,
                    next_attempt_at=CASE WHEN reconciled_revision<excluded.desired_revision THEN 0 ELSE next_attempt_at END")
                    .bind(root).bind(host).bind(selected.revision).execute(&mut *tx).await?;
            }
            count += roots.len();
            selected.roots_done = roots.len() < 16;
            if let Some((root, _)) = roots.last() {
                selected.cursor = Some(root.clone());
            }
        }
        if !selected.retirements_done {
            let transfers: Vec<String> = sqlx::query_scalar(
                "SELECT transfer_id FROM download_retirement_work
                WHERE playlist_id=? AND transfer_id>coalesce(?,'') ORDER BY transfer_id LIMIT 16",
            )
            .bind(&selected.playlist_id)
            .bind(&selected.retirement_cursor)
            .fetch_all(&mut *tx)
            .await?;
            for transfer in &transfers {
                sqlx::query(
                    "UPDATE download_retirement_work SET next_check_at=0 WHERE transfer_id=?",
                )
                .bind(transfer)
                .execute(&mut *tx)
                .await?;
            }
            count += transfers.len();
            selected.retirements_done = transfers.len() < 16;
            if let Some(transfer) = transfers.last() {
                selected.retirement_cursor = Some(transfer.clone());
            }
        }
        let dirty = !selected.roots_done || !selected.retirements_done;
        sqlx::query("UPDATE download_playlist_changes SET dirty=?,cursor=?,retirement_cursor=?,roots_done=?,retirements_done=?,next_dispatch_at=unixepoch()+1 WHERE playlist_id=? AND revision=?")
            .bind(dirty).bind(selected.cursor).bind(selected.retirement_cursor).bind(selected.roots_done).bind(selected.retirements_done)
            .bind(&selected.playlist_id).bind(selected.revision).execute(&mut *tx).await?;
        tx.commit().await?;
        tracing::debug!(playlist_id = %selected.playlist_id, revision = selected.revision, item_count = count, remaining = dirty, "Playlist download changes dispatched");
        Ok(true)
    }
}
