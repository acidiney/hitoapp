use super::snapshot;
use crate::{
    LibraryStorage, Result,
    library_storage::{folder_types::Folder, local_path, root::Root},
};
use std::{collections::HashMap, sync::Arc};
use tokio::sync::Semaphore;

impl LibraryStorage {
    /// Cheap comparison of the folder's files with what the last scan indexed:
    /// names, sizes and modification times only, never checksums. Files that
    /// changed while no watcher was running are found here.
    pub(super) async fn changed_since_last_scan(
        &self,
        folder: &Folder,
        root: &Root,
        slots: &Arc<Semaphore>,
    ) -> Result<bool> {
        let reference: Option<i64> = sqlx::query_scalar(
            "SELECT COALESCE(f.last_scan_at, (SELECT MAX(j.created_at) FROM jobs j WHERE j.folder_id = f.id))
             FROM music_folders f WHERE f.id = ?",
        )
        .bind(&folder.id)
        .fetch_one(&self.store.pool)
        .await?;
        let Some(reference) = reference else {
            return Ok(true);
        };
        let rows: Vec<(String, i64)> = sqlx::query_as(
            "SELECT f.relative_path, f.size_bytes FROM local_files f
             JOIN sources s ON s.id = f.source_id WHERE f.folder_id = ? AND s.ready = 1",
        )
        .bind(&folder.id)
        .fetch_all(&self.store.pool)
        .await?;
        let mut known: HashMap<String, i64> = rows.into_iter().collect();
        let expected = known.len();
        let Ok(permit) = slots.clone().acquire_owned().await else {
            return Ok(false);
        };
        let root = root.clone();
        let outcome = tokio::task::spawn_blocking(move || {
            let _permit = permit;
            let mut changed = false;
            let mut present = 0usize;
            let capped = snapshot::walk(&root, |file| {
                let Ok(encoded) = local_path::encode(file.relative) else {
                    return;
                };
                let newer = file.modified_secs.is_some_and(|time| time >= reference);
                match known.remove(&encoded) {
                    Some(size) => {
                        present += 1;
                        changed |= newer || size != file.size as i64;
                    }
                    None => changed |= newer,
                }
            });
            (changed || (!capped && present < expected), capped)
        })
        .await
        .map_err(|_| crate::Error::Internal("Music folder comparison stopped".into()))?;
        if outcome.1 {
            tracing::warn!(
                event = "folder_watch_file_cap",
                folder_id = %folder.id,
                "Music folder has more files than the start-up comparison reads"
            );
            return Ok(false);
        }
        Ok(outcome.0)
    }
}
