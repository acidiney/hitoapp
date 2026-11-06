use super::{
    LibraryStorage,
    folder_types::Folder,
    inspection::CachedFile,
    lease::Lease,
    local_path, metadata_owner,
    root::Root,
    types::{AUDIO_ALGORITHM, IndexedFile},
    walking::{Paths, paths},
};
use crate::{Error, Result};
use futures_util::{StreamExt, stream::FuturesOrdered};
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};
use tokio::time::Interval;

impl LibraryStorage {
    #[tracing::instrument(level = "info", skip_all, err(Debug), fields(host_device = self.host.as_str(), job_id = %lease.job, folder_id = %lease.folder.id))]
    pub(super) async fn scan(&self, lease: &Lease, root: Root) -> Result<i64> {
        lease.check_running()?;
        tokio::select! {
            biased;
            _ = lease.cancelled() => return Err(super::lease::stopped()),
            result = self.inspector.check_tools() => result?,
        }
        let mut heartbeat = tokio::time::interval(Duration::from_secs(1));
        if self
            .legacy_evidence_pending(&lease.folder.library_id)
            .await?
        {
            let witnesses: Vec<Folder> = sqlx::query_as("SELECT * FROM music_folders WHERE library_id = ? AND host_device = ? AND connected = 1 AND bound = 1 ORDER BY id")
            .bind(&lease.folder.library_id).bind(self.host.as_str()).fetch_all(&self.store.pool).await?;
            for witness in witnesses {
                if let Ok(witness_root) = self.open_scan_root(lease, &witness).await {
                    self.adopt_legacy(lease, &witness, &witness_root, &mut heartbeat)
                        .await?;
                }
            }
        }
        let mut receiver = paths(&root)?;
        let parallelism = self.inspector.parallelism();
        let mut pending = FuturesOrdered::new();
        let mut walking = true;
        let mut count = 0;
        let mut skipped = 0;
        let started = Instant::now();
        while walking || !pending.is_empty() {
            let accepting = walking && pending.len() < parallelism;
            tokio::select! {
                biased;
                _ = lease.cancelled() => return Err(super::lease::stopped()),
                _ = heartbeat.tick() => {
                    lease.renew(&self.store, count).await?;
                    lease.check_folder(&self.store, &lease.folder).await?;
                },
                Some((path, inspected)) = pending.next() => {
                    let path: PathBuf = path;
                    match inspected {
                        Ok(file) => {
                            lease.check_running()?;
                            self.index_file(lease, &root, file).await?;
                            count += 1;
                        }
                        Err(error) if lease.is_stopping() => return Err(error),
                        Err(error @ (Error::Database(_) | Error::InsufficientStorage)) => return Err(error),
                        Err(error) => {
                            self.invalidate_path(lease, &root, path.as_path()).await?;
                            skipped += 1;
                            tracing::warn!(error_code = error.code(), "Skipping unverified audio file");
                        }
                    }
                },
                next = receiver.next(), if accepting => {
                    let next: Option<PathBuf> = next?;
                    match next {
                        Some(path) => {
                            let root = &root;
                            let cached = self.cached_file(lease, &lease.folder, root, path.as_path()).await?;
                            pending.push_back(async move {
                                let inspected = self.inspector.inspect(root, path.as_path(), cached).await;
                                (path, inspected)
                            });
                        }
                        None => walking = false,
                    }
                },
            }
        }
        let elapsed = started.elapsed();
        tracing::info!(
            files_read = count,
            files_skipped = skipped,
            parallelism,
            elapsed_ms = elapsed.as_millis() as u64,
            "Scan inspected"
        );
        let stale = self.reconcile_missing(lease, count).await?;
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
        lease: &Lease,
        folder: &Folder,
        count: i64,
        root: &Root,
        path: &Path,
        heartbeat: &mut Interval,
    ) -> Result<IndexedFile> {
        let inspection = async {
            let cached = self.cached_file(lease, folder, root, path).await?;
            self.inspector.inspect(root, path, cached).await
        };
        tokio::pin!(inspection);
        loop {
            tokio::select! {
                biased;
                _ = lease.cancelled() => return Err(super::lease::stopped()),
                result = &mut inspection => return result,
                _ = heartbeat.tick() => {
                    lease.renew(&self.store, count).await?;
                    lease.check_folder(&self.store, folder).await?;
                },
            }
        }
    }

    async fn cached_file(
        &self,
        lease: &Lease,
        folder: &Folder,
        root: &Root,
        path: &Path,
    ) -> Result<Option<CachedFile>> {
        lease.check_folder(&self.store, folder).await?;
        let relative = relative(root, path)?;
        let cached: Option<CachedFile> = sqlx::query_as(
            "SELECT f.file_checksum AS checksum, f.audio_digest, m.inspection_version AS metadata_version,
                m.title, m.artist, m.duration_ms, m.disc_number, m.track_number, o.tags AS tag_details,
                COALESCE(a.title, 'Unknown album') AS album, COALESCE(a.artist, m.artist) AS album_artist
             FROM local_files f JOIN sources s ON s.id = f.source_id
             JOIN source_metadata m ON m.source_id = s.id LEFT JOIN releases a ON a.id = m.release_id
             LEFT JOIN metadata_observations o ON o.id = m.observation_id
             WHERE f.folder_id = ? AND f.relative_path = ? AND f.scan_root = ? AND s.device_id = ? AND f.inspection_scheme = ?",
        ).bind(&folder.id).bind(relative).bind(root.path().to_string_lossy().as_ref())
            .bind(self.host.as_str()).bind(AUDIO_ALGORITHM).fetch_optional(&self.store.pool).await?;
        Ok(cached)
    }

    pub(super) async fn next_path(
        &self,
        lease: &Lease,
        folder: &Folder,
        count: i64,
        receiver: &mut Paths,
        heartbeat: &mut Interval,
    ) -> Result<Option<PathBuf>> {
        lease.check_folder(&self.store, folder).await?;
        let next = receiver.next();
        tokio::pin!(next);
        loop {
            tokio::select! {
                biased;
                _ = lease.cancelled() => return Err(super::lease::stopped()),
                path = &mut next => return path,
                _ = heartbeat.tick() => {
                    lease.renew(&self.store, count).await?;
                    lease.check_folder(&self.store, folder).await?;
                },
            }
        }
    }

    pub(super) async fn checksum_with_lease(
        &self,
        lease: &Lease,
        folder: &Folder,
        root: &Root,
        path: &Path,
        heartbeat: &mut Interval,
    ) -> Result<String> {
        lease.check_folder(&self.store, folder).await?;
        let checksum = self.inspector.checksum(root, path);
        tokio::pin!(checksum);
        loop {
            tokio::select! {
                biased;
                _ = lease.cancelled() => return Err(super::lease::stopped()),
                result = &mut checksum => return result,
                _ = heartbeat.tick() => {
                    lease.renew(&self.store, 0).await?;
                    lease.check_folder(&self.store, folder).await?;
                },
            }
        }
    }

    async fn invalidate_path(&self, lease: &Lease, root: &Root, path: &Path) -> Result<()> {
        let mut tx = self.store.pool.begin_with("BEGIN IMMEDIATE").await?;
        lease.guard(&mut tx).await?;
        let recording: Option<String> = sqlx::query_scalar(
            "UPDATE sources SET ready = 0 WHERE id IN (SELECT source_id FROM local_files WHERE folder_id = ? AND relative_path = ?) RETURNING recording_id",
        ).bind(&lease.folder.id).bind(relative(root, path)?).fetch_optional(&mut *tx).await?;
        if let Some(recording) = recording {
            metadata_owner::refresh_metadata(&mut tx, &recording).await?;
        }
        tx.commit().await?;
        Ok(())
    }

    async fn reconcile_missing(&self, lease: &Lease, count: i64) -> Result<u64> {
        let mut cursor = String::new();
        let mut stale = 0;
        loop {
            lease.check_running()?;
            lease.renew(&self.store, count).await?;
            let mut tx = self.store.pool.begin_with("BEGIN IMMEDIATE").await?;
            lease.guard(&mut tx).await?;
            let files: Vec<(String, String)> = sqlx::query_as(
                "SELECT f.source_id, s.recording_id FROM local_files f JOIN sources s ON s.id = f.source_id
                 WHERE f.folder_id = ? AND f.source_id > ? AND s.ready = 1 AND f.last_scan_attempt IS NOT ? ORDER BY f.source_id LIMIT 64",
            ).bind(&lease.folder.id).bind(&cursor).bind(&lease.token).fetch_all(&mut *tx).await?;
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
                metadata_owner::refresh_metadata(&mut tx, &recording).await?;
            }
            tx.commit().await?;
        }
        Ok(stale)
    }
}

pub(super) fn relative(root: &Root, path: &Path) -> Result<String> {
    local_path::encode(
        path.strip_prefix(root.path())
            .map_err(|_| Error::Forbidden)?,
    )
}
