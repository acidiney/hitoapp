use super::{DownloadRequest, Downloads, PlaylistDownloadReceipt, roots::identity};
use crate::{Actor, Error, Result};
use sha2::{Digest, Sha256};
use uuid::Uuid;

impl Downloads {
    #[tracing::instrument(level = "info", skip_all, err(Debug), fields(account_id = actor.id(), request_id = %request.request_id))]
    pub async fn create_playlist(
        &self,
        actor: &Actor,
        playlist: &str,
        request: DownloadRequest,
    ) -> Result<PlaylistDownloadReceipt> {
        let started = std::time::Instant::now();
        let playlist = identity(playlist)?;
        let digest = format!(
            "{:x}",
            Sha256::digest(format!(
                "hito.download.playlist.v1|{playlist}|{}",
                self.host.as_str()
            ))
        );
        let mut tx = self.store.pool.begin_with("BEGIN IMMEDIATE").await?;
        actor.check_session_on(&mut tx).await?;
        let previous: Option<(String, String)> = sqlx::query_as("SELECT request_digest,retention_id FROM download_playlist_receipts WHERE account_id=? AND request_id=?")
            .bind(actor.id()).bind(request.request_id.to_string()).fetch_optional(&mut *tx).await?;
        if let Some((previous_digest, retention_id)) = previous {
            if previous_digest != digest {
                return Err(Error::Conflict(
                    "This playlist download request was already used for another playlist or host"
                        .into(),
                ));
            }
            tx.commit().await?;
            tracing::info!(retention_id = %retention_id, outcome = "replayed", "Playlist download request retained");
            return Ok(PlaylistDownloadReceipt {
                request_id: request.request_id,
                retention_id,
                playlist_id: playlist,
            });
        }
        self.files.ensure_supported()?;
        let owned: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM playlists WHERE id=? AND user_id=? AND deleted=0)",
        )
        .bind(&playlist)
        .bind(actor.id())
        .fetch_one(&mut *tx)
        .await?;
        if !owned {
            return Err(Error::NotFound);
        }
        let existing: Option<String> = sqlx::query_scalar("SELECT id FROM download_retentions
            WHERE account_id=? AND playlist_id=? AND host_device=? AND kind='playlist' AND state='active'")
            .bind(actor.id()).bind(&playlist).bind(self.host.as_str()).fetch_optional(&mut *tx).await?;
        let retention_id = if let Some(existing) = existing {
            existing
        } else {
            let root = Uuid::new_v4().to_string();
            sqlx::query("INSERT INTO download_retentions(id,account_id,host_device,kind,playlist_id,state) VALUES(?,?,?,'playlist',?,'active')")
                .bind(&root).bind(actor.id()).bind(self.host.as_str()).bind(&playlist).execute(&mut *tx).await?;
            sqlx::query("INSERT OR IGNORE INTO download_playlist_changes(playlist_id) VALUES(?)")
                .bind(&playlist)
                .execute(&mut *tx)
                .await?;
            sqlx::query(
                "INSERT INTO download_playlist_work(retention_id,host_device,desired_revision)
                SELECT ?,?,revision FROM download_playlist_changes WHERE playlist_id=?",
            )
            .bind(&root)
            .bind(self.host.as_str())
            .bind(&playlist)
            .execute(&mut *tx)
            .await?;
            root
        };
        sqlx::query("INSERT INTO download_playlist_receipts(account_id,request_id,request_digest,retention_id) VALUES(?,?,?,?)")
            .bind(actor.id()).bind(request.request_id.to_string()).bind(digest).bind(&retention_id).execute(&mut *tx).await?;
        actor.check_session_on(&mut tx).await?;
        tx.commit().await?;
        tracing::info!(retention_id = %retention_id, playlist_id = %playlist, outcome = "retained", elapsed_ms = started.elapsed().as_millis() as u64, "Playlist download requested");
        Ok(PlaylistDownloadReceipt {
            request_id: request.request_id,
            retention_id,
            playlist_id: playlist,
        })
    }
}
