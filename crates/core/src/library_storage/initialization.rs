use super::{LibraryStorage, folder_types::Folder, folders};
use crate::Result;
use std::collections::HashMap;

impl LibraryStorage {
    pub(crate) async fn initialize(&self) -> Result<()> {
        let mut tx = self.store.pool.begin_with("BEGIN IMMEDIATE").await?;
        let folders: Vec<Folder> = sqlx::query_as(
            "SELECT * FROM music_folders WHERE host_device = ? ORDER BY library_id, id",
        )
        .bind(self.host.as_str())
        .fetch_all(&mut *tx)
        .await?;
        let grants = self.directories.grants();
        let mut counts = HashMap::new();
        for folder in &folders {
            *counts.entry(&folder.library_id).or_insert(0usize) += 1;
        }
        for folder in &folders {
            let grant = folder
                .directory_id
                .as_ref()
                .and_then(|id| grants.list.iter().find(|grant| &grant.id == id))
                .or_else(|| {
                    if folder.directory_id.is_none() {
                        grants
                            .list
                            .iter()
                            .find(|grant| grant.root.to_string_lossy() == folder.root_path)
                            .or_else(|| {
                                if self.directories.legacy.is_some()
                                    && counts.get(&folder.library_id) == Some(&1)
                                {
                                    grants.list.first()
                                } else {
                                    None
                                }
                            })
                    } else {
                        None
                    }
                });
            let bound = grant.is_some();
            let moved = grant.is_some_and(|grant| grant.root.to_string_lossy() != folder.root_path);
            if (folder.bound && !bound) || moved {
                sqlx::query("UPDATE music_folders SET bound = ?, generation = generation + 1, revision = revision + 1 WHERE id = ?")
                    .bind(bound).bind(&folder.id).execute(&mut *tx).await?;
                folders::withdraw(&mut tx, &folder.id).await?;
            }
            if let Some(grant) = grant {
                sqlx::query("UPDATE music_folders SET directory_id = ?, bound = 1, root_path = ? WHERE id = ?")
                    .bind(&grant.id).bind(grant.root.to_string_lossy().as_ref()).bind(&folder.id).execute(&mut *tx).await?;
            } else if folder.directory_id.is_none() {
                folders::withdraw(&mut tx, &folder.id).await?;
            }
        }
        tx.commit().await?;
        tracing::debug!(
            event = "music_folder_bindings_loaded",
            count = folders.len(),
            "Trusted music folder bindings loaded"
        );
        Ok(())
    }
}
