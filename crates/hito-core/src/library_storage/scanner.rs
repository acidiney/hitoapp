use super::{
    LibraryStorage,
    inspection::CachedFile,
    observations,
    root::Root,
    types::{AUDIO_ALGORITHM, IndexedFile},
    walking::paths,
};
use crate::{Error, Result};
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
    time::Duration,
};
use tokio::{sync::mpsc::Receiver, time::Interval};

impl LibraryStorage {
    #[tracing::instrument(level = "info", skip_all, err(Debug), fields(host_device = self.host.as_str()))]
    pub(super) async fn scan(&self, job: &str, library: &str) -> Result<i64> {
        self.inspector.check_tools().await?;
        if self.directories.legacy.is_none() {
            return Err(Error::Invalid(
                "Connect a music folder before scanning".into(),
            ));
        }
        let root = self.directories.open("legacy-music").await?;
        let mut heartbeat = tokio::time::interval(Duration::from_secs(30));
        self.adopt_legacy(job, library, &root, &mut heartbeat)
            .await?;
        let (mut receiver, reader) = paths(&root);
        let mut count = 0i64;
        let mut skipped = 0i64;
        while let Some(path) = self
            .next_path(job, count, &mut receiver, &mut heartbeat)
            .await?
        {
            let result = self
                .inspect_with_lease(job, library, count, &root, &path, &mut heartbeat)
                .await;
            match result {
                Ok(file) => {
                    self.index_file(job, library, root.path(), file).await?;
                    count += 1;
                }
                Err(error @ Error::Database(_)) => return Err(error),
                Err(error) => {
                    self.invalidate_path(library, &path).await?;
                    skipped += 1;
                    tracing::warn!(error_code = error.code(), "Skipping unverified audio file");
                }
            }
        }
        reader
            .await
            .map_err(|_| Error::Internal("Music folder reader stopped".into()))??;
        let stale = self
            .reconcile_missing(job, library, root.path(), count)
            .await?;
        tracing::info!(
            files_read = count,
            files_skipped = skipped,
            stale_sources = stale,
            "Scan reconciled"
        );
        Ok(count)
    }

    pub(super) async fn inspect_with_lease(
        &self,
        job: &str,
        library: &str,
        count: i64,
        root: &Root,
        path: &Path,
        heartbeat: &mut tokio::time::Interval,
    ) -> Result<IndexedFile> {
        let cached: Option<CachedFile> = sqlx::query_as(
            "SELECT f.file_checksum AS checksum, f.audio_digest, m.title, m.artist, m.duration_ms,
                COALESCE(a.title, 'Unknown album') AS album, COALESCE(a.artist, m.artist) AS album_artist
             FROM local_files f JOIN sources s ON s.id = f.source_id
             JOIN source_metadata m ON m.source_id = s.id LEFT JOIN releases a ON a.id = m.release_id
             WHERE f.library_id = ? AND f.path = ? AND f.scan_root = ? AND s.device_id = ? AND f.inspection_scheme = ?",
        ).bind(library).bind(path.to_string_lossy().as_ref()).bind(root.path().to_string_lossy().as_ref())
            .bind(self.host.as_str()).bind(AUDIO_ALGORITHM).fetch_optional(&self.store.pool).await?;
        let inspection = self.inspector.inspect(root, path, cached);
        tokio::pin!(inspection);
        loop {
            tokio::select! {
                result = &mut inspection => return result,
                _ = heartbeat.tick() => self.renew_lease(job, count).await?,
            }
        }
    }

    async fn renew_lease(&self, job: &str, count: i64) -> Result<()> {
        sqlx::query("UPDATE jobs SET lease_until = unixepoch() + 300, progress = ? WHERE id = ?")
            .bind(count)
            .bind(job)
            .execute(&self.store.pool)
            .await?;
        tracing::debug!(files_read = count, "Scan lease renewed");
        Ok(())
    }

    pub(super) async fn next_path(
        &self,
        job: &str,
        count: i64,
        receiver: &mut Receiver<PathBuf>,
        heartbeat: &mut Interval,
    ) -> Result<Option<PathBuf>> {
        loop {
            tokio::select! {
                path = receiver.recv() => return Ok(path),
                _ = heartbeat.tick() => self.renew_lease(job, count).await?,
            }
        }
    }

    pub(super) async fn checksum_with_lease(
        &self,
        job: &str,
        root: &Root,
        path: &Path,
        heartbeat: &mut Interval,
    ) -> Result<String> {
        let checksum = self.inspector.checksum(root, path);
        tokio::pin!(checksum);
        loop {
            tokio::select! {
                result = &mut checksum => return result,
                _ = heartbeat.tick() => self.renew_lease(job, 0).await?,
            }
        }
    }

    async fn invalidate_path(&self, library: &str, path: &Path) -> Result<()> {
        let mut tx = self.store.pool.begin_with("BEGIN IMMEDIATE").await?;
        let recording: Option<String> = sqlx::query_scalar(
            "UPDATE sources SET ready = 0 WHERE device_id = ? AND id IN (
             SELECT source_id FROM local_files WHERE library_id = ? AND path = ?) RETURNING recording_id",
        ).bind(self.host.as_str()).bind(library).bind(path.to_string_lossy().as_ref())
            .fetch_optional(&mut *tx).await?;
        if let Some(recording) = recording {
            observations::refresh_metadata(&mut tx, &recording).await?;
        }
        tx.commit().await?;
        Ok(())
    }

    async fn reconcile_missing(
        &self,
        job: &str,
        library: &str,
        root: &Path,
        count: i64,
    ) -> Result<u64> {
        let mut cursor = String::new();
        let mut stale = 0;
        loop {
            self.renew_lease(job, count).await?;
            let mut tx = self.store.pool.begin_with("BEGIN IMMEDIATE").await?;
            let files: Vec<(String, String)> = sqlx::query_as(
                "SELECT f.source_id, s.recording_id FROM local_files f JOIN sources s ON s.id = f.source_id
                 WHERE f.library_id = ? AND f.source_id > ? AND s.device_id = ?
                   AND s.ready = 1 AND (f.scan_root != ? OR f.last_scan_job IS NOT ?) ORDER BY f.source_id LIMIT 64",
            ).bind(library).bind(&cursor).bind(self.host.as_str())
                .bind(root.to_string_lossy().as_ref()).bind(job).fetch_all(&mut *tx).await?;
            if files.is_empty() {
                break;
            }
            let mut recordings = BTreeSet::new();
            for (source, recording) in files {
                cursor = source.clone();
                stale += sqlx::query("UPDATE sources SET ready = 0 WHERE id = ?")
                    .bind(source)
                    .execute(&mut *tx)
                    .await?
                    .rows_affected();
                recordings.insert(recording);
            }
            for recording in recordings {
                observations::refresh_metadata(&mut tx, &recording).await?;
            }
            tx.commit().await?;
        }
        Ok(stale)
    }
}
