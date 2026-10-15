use super::{
    LibraryStorage,
    lease::Lease,
    metadata_owner, observations,
    resolution::{self, LocalIdentity},
    root::Root,
    scanner,
    types::{AUDIO_ALGORITHM, IndexedFile},
};
use crate::Result;
use uuid::Uuid;

impl LibraryStorage {
    #[tracing::instrument(level = "trace", skip_all, err(Debug), fields(host_device = self.host.as_str()))]
    pub(super) async fn index_file(
        &self,
        lease: &Lease,
        root: &Root,
        file: IndexedFile,
    ) -> Result<()> {
        let mut tx = self.store.pool.begin_with("BEGIN IMMEDIATE").await?;
        lease.guard(&mut tx).await?;
        let library = &lease.folder.library_id;
        let relative = scanner::relative(root, &file.path)?;
        let path = file.path.to_string_lossy();
        let previous: Option<LocalIdentity> = sqlx::query_as(
            "SELECT s.id AS source, s.recording_id AS recording, s.device_id AS device, f.file_checksum AS checksum
             FROM local_files f JOIN sources s ON s.id = f.source_id WHERE f.folder_id = ? AND f.relative_path = ?",
        ).bind(&lease.folder.id).bind(&relative).fetch_optional(&mut *tx).await?;
        resolution::check_host(previous.as_ref(), self.host.as_str())?;
        let release = observations::release(&mut tx, library, root.path(), &file).await?;
        let recording =
            resolution::resolve(&mut tx, library, &release, &file, previous.as_ref()).await?;
        let source = match previous {
            Some(previous) if previous.recording == recording => previous.source,
            Some(previous) => {
                sqlx::query("UPDATE sources SET ready = 0 WHERE id = ?")
                    .bind(&previous.source)
                    .execute(&mut *tx)
                    .await?;
                sqlx::query("DELETE FROM local_files WHERE source_id = ?")
                    .bind(previous.source)
                    .execute(&mut *tx)
                    .await?;
                metadata_owner::refresh_metadata(&mut tx, &previous.recording).await?;
                tracing::debug!(
                    event = "local_audio_replaced",
                    "Previous source retained as unavailable"
                );
                Uuid::new_v4().to_string()
            }
            None => Uuid::new_v4().to_string(),
        };
        sqlx::query(
            "INSERT INTO sources(id, recording_id, provider, provider_id, device_id) VALUES (?, ?, 'local', ?, ?)
             ON CONFLICT(id) DO UPDATE SET ready = 1",
        ).bind(&source).bind(&recording).bind(&source).bind(self.host.as_str()).execute(&mut *tx).await?;
        sqlx::query(
            "INSERT INTO local_files(source_id, library_id, folder_id, relative_path, path, scan_root, size_bytes, file_checksum, last_scan_attempt, inspection_scheme, audio_digest)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT(source_id) DO UPDATE SET path = excluded.path, size_bytes = excluded.size_bytes,
                scan_root = excluded.scan_root, file_checksum = excluded.file_checksum, last_scan_attempt = excluded.last_scan_attempt,
                inspection_scheme = excluded.inspection_scheme, audio_digest = excluded.audio_digest",
        ).bind(&source).bind(library).bind(&lease.folder.id).bind(relative).bind(path.as_ref()).bind(root.path().to_string_lossy().as_ref())
            .bind(file.size).bind(&file.checksum).bind(&lease.token).bind(AUDIO_ALGORITHM)
            .bind(file.audio.as_ref().map(|audio| &audio.digest)).execute(&mut *tx).await?;
        observations::remember(&mut tx, &recording, &source, &release, &file).await?;
        tx.commit().await?;
        tracing::trace!(size_bytes = file.size, recording_id = %recording, source_id = %source, "Local audio indexed");
        Ok(())
    }
}
