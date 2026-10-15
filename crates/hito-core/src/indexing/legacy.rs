use super::{
    Indexer, observations,
    root::Root,
    types::{AUDIO_ALGORITHM, IndexedFile},
    walking,
};
use crate::{Error, Result};
use sqlx::{Sqlite, Transaction};
use std::path::Path;
use tokio::time::Interval;

impl Indexer {
    pub(super) async fn adopt_legacy(
        &self,
        job: &str,
        library: &str,
        root: &Root,
        heartbeat: &mut Interval,
    ) -> Result<()> {
        let pending: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM recordings r WHERE r.library_id = ?
             AND EXISTS(SELECT 1 FROM sources s WHERE s.recording_id = r.id AND s.device_id = ?)
             AND NOT EXISTS(SELECT 1 FROM recording_evidence e WHERE e.recording_id = r.id AND e.algorithm = ?)
             AND NOT EXISTS(SELECT 1 FROM local_files f JOIN sources s ON s.id = f.source_id
                            WHERE s.recording_id = r.id AND f.inspection_scheme = ?))",
        ).bind(library).bind(self.host.as_str()).bind(AUDIO_ALGORITHM).bind(AUDIO_ALGORITHM)
            .fetch_one(&self.store.pool).await?;
        if !pending {
            return Ok(());
        }
        let (mut paths, reader) = walking::paths(root);
        let mut adopted = 0;
        while let Some(path) = self.next_path(job, 0, &mut paths, heartbeat).await? {
            let checksum = match self.checksum_with_lease(job, root, &path, heartbeat).await {
                Ok(checksum) => checksum,
                Err(error @ Error::Database(_)) => return Err(error),
                Err(_) => continue,
            };
            let candidate: bool = sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM recordings r WHERE r.library_id = ? AND r.initial_file_checksum = ?
                 AND EXISTS(SELECT 1 FROM sources s WHERE s.recording_id = r.id AND s.device_id = ?)
                 AND NOT EXISTS(SELECT 1 FROM recording_evidence e WHERE e.recording_id = r.id AND e.algorithm = ?)
                 AND NOT EXISTS(SELECT 1 FROM local_files f JOIN sources s ON s.id = f.source_id
                                WHERE s.recording_id = r.id AND f.inspection_scheme = ?))",
            ).bind(library).bind(&checksum).bind(self.host.as_str()).bind(AUDIO_ALGORITHM).bind(AUDIO_ALGORITHM)
                .fetch_one(&self.store.pool).await?;
            if !candidate {
                continue;
            }
            let file = match self
                .inspect_with_lease(job, library, 0, root, &path, heartbeat)
                .await
            {
                Ok(file) => file,
                Err(error @ Error::Database(_)) => return Err(error),
                Err(_) => continue,
            };
            if file.checksum != checksum {
                continue;
            }
            let Some(audio) = &file.audio else {
                continue;
            };
            let mut tx = self.store.pool.begin_with("BEGIN IMMEDIATE").await?;
            adopted += sqlx::query(
                "INSERT OR IGNORE INTO recording_evidence(recording_id, algorithm, digest)
                 SELECT r.id, ?, ? FROM recordings r
                 WHERE r.library_id = ? AND r.initial_file_checksum = ?
                   AND EXISTS(SELECT 1 FROM sources s WHERE s.recording_id = r.id AND s.device_id = ?)",
            ).bind(audio.algorithm).bind(&audio.digest).bind(library).bind(&checksum)
                .bind(self.host.as_str())
                .execute(&mut *tx).await?.rows_affected();
            self.cache_legacy_witness(&mut tx, library, root.path(), &file)
                .await?;
            tx.commit().await?;
        }
        reader
            .await
            .map_err(|_| Error::Internal("Music folder reader stopped".into()))??;
        tracing::debug!(
            event = "legacy_audio_evidence_adopted",
            recordings = adopted,
            "Legacy identity evidence checked"
        );
        Ok(())
    }

    async fn cache_legacy_witness(
        &self,
        tx: &mut Transaction<'_, Sqlite>,
        library: &str,
        root: &Path,
        file: &IndexedFile,
    ) -> Result<()> {
        let owner: Option<(String, String)> = sqlx::query_as(
            "SELECT f.source_id, s.recording_id FROM local_files f JOIN sources s ON s.id = f.source_id
             WHERE f.library_id = ? AND f.path = ? AND f.scan_root = ? AND s.device_id = ? AND f.file_checksum = ?",
        ).bind(library).bind(file.path.to_string_lossy().as_ref()).bind(root.to_string_lossy().as_ref()).bind(self.host.as_str()).bind(&file.checksum).fetch_optional(&mut **tx).await?;
        let Some((source, recording)) = owner else {
            return Ok(());
        };
        let release = observations::release(tx, library, root, file).await?;
        observations::remember(tx, &recording, &source, &release, file).await?;
        sqlx::query(
            "UPDATE local_files SET inspection_scheme = ?, audio_digest = ? WHERE source_id = ?",
        )
        .bind(AUDIO_ALGORITHM)
        .bind(file.audio.as_ref().map(|audio| &audio.digest))
        .bind(source)
        .execute(&mut **tx)
        .await?;
        Ok(())
    }
}
