use super::{
    LibraryStorage,
    folder_types::Folder,
    lease::Lease,
    observations,
    root::Root,
    scanner,
    types::{AUDIO_ALGORITHM, IndexedFile},
    walking,
};
use crate::{Error, Result};
use sqlx::{Sqlite, Transaction};
use tokio::time::Interval;

impl LibraryStorage {
    pub(super) async fn legacy_evidence_pending(&self, library: &str) -> Result<bool> {
        Ok(sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM recordings r WHERE r.library_id = ?
             AND EXISTS(SELECT 1 FROM sources s WHERE s.recording_id = r.id AND s.device_id = ?)
             AND NOT EXISTS(SELECT 1 FROM recording_evidence e WHERE e.recording_id = r.id AND e.algorithm = ?)
             AND NOT EXISTS(SELECT 1 FROM local_files f JOIN sources s ON s.id = f.source_id
                            WHERE s.recording_id = r.id AND f.inspection_scheme = ?))",
        ).bind(library).bind(self.host.as_str()).bind(AUDIO_ALGORITHM).bind(AUDIO_ALGORITHM)
            .fetch_one(&self.store.pool).await?)
    }

    pub(super) async fn adopt_legacy(
        &self,
        lease: &Lease,
        witness: &Folder,
        root: &Root,
        heartbeat: &mut Interval,
    ) -> Result<()> {
        let mut paths = walking::paths(root)?;
        let result = self
            .adopt_paths(lease, witness, root, heartbeat, &mut paths)
            .await;
        drop(paths);
        let adopted = match result {
            Err(error @ Error::Conflict(_)) => {
                lease.renew(&self.store, 0).await?;
                match lease.check_folder(&self.store, witness).await {
                    Err(Error::Conflict(_)) => {
                        tracing::debug!(folder_id = %witness.id, "Removed legacy witness inspection stopped");
                        return Ok(());
                    }
                    Err(error) => return Err(error),
                    Ok(()) => {}
                }
                return Err(error);
            }
            result => result?,
        };
        tracing::debug!(
            event = "legacy_audio_evidence_adopted",
            recordings = adopted,
            "Legacy identity evidence checked"
        );
        Ok(())
    }

    async fn adopt_paths(
        &self,
        lease: &Lease,
        witness: &Folder,
        root: &Root,
        heartbeat: &mut Interval,
        paths: &mut walking::Paths,
    ) -> Result<u64> {
        let library = &lease.folder.library_id;
        let mut adopted = 0;
        while let Some(path) = self.next_path(lease, witness, 0, paths, heartbeat).await? {
            let checksum = match self
                .checksum_with_lease(lease, witness, root, &path, heartbeat)
                .await
            {
                Ok(checksum) => checksum,
                Err(error @ (Error::Database(_) | Error::Conflict(_))) => return Err(error),
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
                .inspect_with_lease(lease, witness, 0, root, &path, heartbeat)
                .await
            {
                Ok(file) => file,
                Err(error @ (Error::Database(_) | Error::Conflict(_))) => return Err(error),
                Err(_) => continue,
            };
            if file.checksum != checksum {
                continue;
            }
            let Some(audio) = &file.audio else {
                continue;
            };
            let mut tx = self.store.pool.begin_with("BEGIN IMMEDIATE").await?;
            lease.guard(&mut tx).await?;
            let active: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM music_folders WHERE id = ? AND generation = ? AND connected = 1 AND bound = 1)").bind(&witness.id).bind(witness.generation).fetch_one(&mut *tx).await?;
            if !active {
                continue;
            }
            adopted += sqlx::query(
                "INSERT OR IGNORE INTO recording_evidence(recording_id, algorithm, digest)
                 SELECT r.id, ?, ? FROM recordings r
                 WHERE r.library_id = ? AND r.initial_file_checksum = ?
                   AND EXISTS(SELECT 1 FROM sources s WHERE s.recording_id = r.id AND s.device_id = ?)",
            ).bind(audio.algorithm).bind(&audio.digest).bind(library).bind(&checksum)
                .bind(self.host.as_str())
                .execute(&mut *tx).await?.rows_affected();
            self.cache_legacy_witness(&mut tx, witness, root, &file)
                .await?;
            tx.commit().await?;
        }
        Ok(adopted)
    }

    async fn cache_legacy_witness(
        &self,
        tx: &mut Transaction<'_, Sqlite>,
        witness: &Folder,
        root: &Root,
        file: &IndexedFile,
    ) -> Result<()> {
        let owner: Option<(String, String)> = sqlx::query_as(
            "SELECT f.source_id, s.recording_id FROM local_files f JOIN sources s ON s.id = f.source_id
             WHERE f.folder_id = ? AND f.relative_path = ? AND f.scan_root = ? AND s.device_id = ? AND f.file_checksum = ?",
        ).bind(&witness.id).bind(scanner::relative(root, &file.path)?).bind(root.path().to_string_lossy().as_ref()).bind(self.host.as_str()).bind(&file.checksum).fetch_optional(&mut **tx).await?;
        let Some((source, recording)) = owner else {
            return Ok(());
        };
        let release = observations::release(tx, &witness.library_id, root.path(), file).await?;
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
