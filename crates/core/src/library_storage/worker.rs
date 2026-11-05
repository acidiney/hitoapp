use super::{LibraryStorage, folder_types::Folder, lease::Lease, metadata_owner};
use crate::{Error, Result};
use uuid::Uuid;

impl LibraryStorage {
    #[tracing::instrument(level = "info", skip_all, err(Debug))]
    pub async fn finish_indexing(&self) -> Result<()> {
        let started = std::time::Instant::now();
        self.inspector.settle().await?;
        tracing::info!(
            event = "indexing_cleanup_finished",
            outcome = "retired",
            elapsed_ms = started.elapsed().as_millis() as u64,
            "Media inspection cleanup finished"
        );
        Ok(())
    }

    #[tracing::instrument(level = "debug", skip_all, err(Debug), fields(host_device = self.host.as_str()))]
    pub async fn run_next_job(&self) -> Result<bool> {
        self.run_next_job_until(std::future::pending()).await
    }

    #[tracing::instrument(level = "debug", skip_all, err(Debug), fields(host_device = self.host.as_str()))]
    pub async fn run_next_job_until(
        &self,
        shutdown: impl std::future::Future<Output = ()> + Send,
    ) -> Result<bool> {
        let (stop, stopped) = tokio::sync::watch::channel(false);
        let (done, finished) = tokio::sync::oneshot::channel::<()>();
        let job = async move {
            let result = self.run_job(stopped).await;
            drop(done);
            result
        };
        let cancellation = async move {
            tokio::select! {
                _ = shutdown => {stop.send_replace(true);},
                _ = finished => {},
            }
        };
        let (result, ()) = tokio::join!(job, cancellation);
        result
    }

    async fn run_job(&self, stop: tokio::sync::watch::Receiver<bool>) -> Result<bool> {
        let mut tx = self.store.pool.begin_with("BEGIN IMMEDIATE").await?;
        let id: Option<String> = sqlx::query_scalar(
            "SELECT j.id FROM jobs j JOIN music_folders f ON f.id = j.folder_id
             WHERE f.host_device = ? AND f.connected = 1 AND f.bound = 1 AND j.folder_generation = f.generation
               AND (j.state = 'queued' OR (j.state = 'running' AND j.lease_until < unixepoch()))
             ORDER BY j.rowid LIMIT 1",
        ).bind(self.host.as_str()).fetch_optional(&mut *tx).await?;
        let Some(id) = id else {
            tx.rollback().await?;
            return Ok(false);
        };
        let folder: Folder = sqlx::query_as(
            "SELECT f.* FROM music_folders f JOIN jobs j ON j.folder_id = f.id WHERE j.id = ?",
        )
        .bind(&id)
        .fetch_one(&mut *tx)
        .await?;
        let token = Uuid::new_v4().to_string();
        sqlx::query("UPDATE jobs SET state = 'running', lease_until = unixepoch() + 300, lease_token = ?, progress = 0, error = NULL WHERE id = ?")
            .bind(&token).bind(&id).execute(&mut *tx).await?;
        tx.commit().await?;
        let lease = Lease {
            job: id,
            token,
            folder,
            stop,
        };
        tracing::info!(job_id = %lease.job, folder_id = %lease.folder.id, generation = lease.folder.generation, "Scan lease acquired");
        let result = self.scan_folder(&lease).await;
        let outcome = if lease.is_stopping() {
            None
        } else {
            Some(result)
        };
        let mut tx = self.store.pool.begin_with("BEGIN IMMEDIATE").await?;
        if let Err(error) = lease.guard(&mut tx).await {
            if !matches!(error, Error::Conflict(_)) {
                return Err(error);
            }
            tracing::info!(job_id = %lease.job, event = "scan_superseded", "Scan attempt no longer owns its target");
            tx.rollback().await?;
            return Ok(true);
        }
        match outcome {
            Some(Ok(count)) => {
                sqlx::query("UPDATE jobs SET state = 'complete', progress = ?, lease_until = NULL, lease_token = NULL WHERE id = ? AND lease_token = ?")
                    .bind(count).bind(&lease.job).bind(&lease.token).execute(&mut *tx).await?;
                sqlx::query("UPDATE music_folders SET last_scan_at = unixepoch() WHERE id = ? AND generation = ?")
                    .bind(&lease.folder.id).bind(lease.folder.generation).execute(&mut *tx).await?;
                crate::artist_catalog::automation::request_for_library(
                    &mut tx,
                    &lease.folder.library_id,
                )
                .await?;
                tracing::info!(job_id = %lease.job, files_read = count, "Scan completed");
            }
            Some(Err(error)) => {
                let message = if matches!(error, Error::MediaToolsMissing) {
                    super::types::MEDIA_TOOLS_MISSING_MESSAGE
                } else {
                    "Music scan failed. Check the connected folder."
                };
                sqlx::query("UPDATE jobs SET state = 'failed', error = ?, lease_until = NULL, lease_token = NULL WHERE id = ? AND lease_token = ?")
                    .bind(message).bind(&lease.job).bind(&lease.token).execute(&mut *tx).await?;
                tracing::warn!(job_id = %lease.job, error_code = error.code(), "Scan failed");
            }
            None => {
                sqlx::query("UPDATE jobs SET state = 'queued', lease_until = NULL, lease_token = NULL WHERE id = ? AND lease_token = ?")
                    .bind(&lease.job).bind(&lease.token).execute(&mut *tx).await?;
                tracing::info!(event = "scan_interrupted", job_id = %lease.job, "Interrupted scan queued for restart");
            }
        }
        tx.commit().await?;
        Ok(true)
    }

    async fn scan_folder(&self, lease: &Lease) -> Result<i64> {
        let root = match self.open_scan_root(lease, &lease.folder).await {
            Ok(root) => root,
            Err(error) => {
                if lease.is_stopping() {
                    return Err(error);
                }
                let mut tx = self.store.pool.begin_with("BEGIN IMMEDIATE").await?;
                lease.guard(&mut tx).await?;
                sqlx::query("UPDATE sources SET ready = 0 WHERE id IN (SELECT source_id FROM local_files WHERE folder_id = ?)")
                    .bind(&lease.folder.id).execute(&mut *tx).await?;
                metadata_owner::refresh_folder_metadata(&mut tx, &lease.folder.id).await?;
                tx.commit().await?;
                return Err(error);
            }
        };
        self.scan(lease, root).await
    }

    pub(super) async fn open_scan_root(
        &self,
        lease: &Lease,
        folder: &Folder,
    ) -> Result<super::Root> {
        lease.check_folder(&self.store, folder).await?;
        let selection = folder.directory().ok_or(Error::NotFound)?;
        let opening = self.directories.open_selected(&selection);
        tokio::pin!(opening);
        let mut heartbeat = tokio::time::interval(std::time::Duration::from_secs(1));
        loop {
            tokio::select! {
                biased;
                _ = lease.cancelled() => return Err(super::lease::stopped()),
                root = &mut opening => return root,
                _ = heartbeat.tick() => {
                    lease.renew(&self.store, 0).await?;
                    lease.check_folder(&self.store, folder).await?;
                }
            }
        }
    }
}
