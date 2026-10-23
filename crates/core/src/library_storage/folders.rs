use super::{
    ConnectFolder, LibraryStorage, MusicFolder, folder_types::Folder, metadata_owner, service,
};
use crate::{Actor, Error, Result, identity::library_access};
use sqlx::{Sqlite, Transaction};
use uuid::Uuid;

impl LibraryStorage {
    #[tracing::instrument(level = "info", skip_all, err(Debug), fields(account_id = actor.id(), folder_id))]
    pub async fn remove_folder(
        &self,
        actor: &Actor,
        id: &str,
        expected_revision: i64,
    ) -> Result<()> {
        actor.require_admin()?;
        let mut tx = self.store.pool.begin_with("BEGIN IMMEDIATE").await?;
        let folder: Folder =
            sqlx::query_as("SELECT * FROM music_folders WHERE id = ? AND host_device = ?")
                .bind(id)
                .bind(self.host.as_str())
                .fetch_optional(&mut *tx)
                .await?
                .ok_or(Error::NotFound)?;
        library_access(&mut tx, actor, &folder.library_id).await?;
        tracing::Span::current().record("folder_id", &folder.id);
        if !folder.connected {
            return Ok(());
        }
        if folder.revision != expected_revision {
            return Err(Error::Conflict(
                "This folder changed. Refresh its settings and try again".into(),
            ));
        }
        sqlx::query("UPDATE music_folders SET connected = 0, generation = generation + 1, revision = revision + 1 WHERE id = ?")
            .bind(id).execute(&mut *tx).await?;
        withdraw(&mut tx, id).await?;
        tx.commit().await?;
        tracing::info!(
            event = "music_folder_removed",
            folder_id = id,
            "Music folder disconnected; originals retained"
        );
        Ok(())
    }

    #[tracing::instrument(level = "debug", skip_all, err(Debug), fields(account_id = actor.id(), library_id))]
    pub async fn folders(&self, actor: &Actor, library: &str) -> Result<Vec<MusicFolder>> {
        actor.require_admin()?;
        let mut connection = self.store.pool.acquire().await?;
        library_access(&mut connection, actor, library).await?;
        crate::identity::shared_library(&mut connection, library).await?;
        tracing::Span::current().record("library_id", library);
        let rows: Vec<Folder> = sqlx::query_as("SELECT * FROM music_folders WHERE library_id = ? AND host_device = ? AND connected = 1 ORDER BY name, id")
            .bind(library).bind(self.host.as_str()).fetch_all(&mut *connection).await?;
        drop(connection);
        let mut pending = tokio::task::JoinSet::new();
        for (index, folder) in rows.into_iter().enumerate() {
            let storage = self.clone();
            let actor = actor.clone();
            pending.spawn(async move {
                Ok::<_, Error>((index, storage.folder_summary(&actor, folder).await?))
            });
        }
        let mut result = Vec::new();
        while let Some(folder) = pending.join_next().await {
            result.push(
                folder
                    .map_err(|_| Error::Internal("Music folder status check stopped".into()))??,
            );
        }
        result.sort_unstable_by_key(|(index, _)| *index);
        Ok(result.into_iter().map(|(_, folder)| folder).collect())
    }

    #[tracing::instrument(level = "info", skip_all, err(Debug), fields(account_id = actor.id(), library_id))]
    pub async fn connect_folder(
        &self,
        actor: &Actor,
        library: &str,
        request: ConnectFolder,
    ) -> Result<MusicFolder> {
        actor.require_admin()?;
        let mut connection = self.store.pool.acquire().await?;
        library_access(&mut connection, actor, library).await?;
        crate::identity::shared_library(&mut connection, library).await?;
        tracing::Span::current().record("library_id", library);
        drop(connection);
        let root = self.directories.open_selected(&request.directory).await?;
        let grant = self
            .directories
            .grants
            .iter()
            .find(|grant| grant.id == request.directory.directory_id)
            .ok_or(Error::NotFound)?;
        let relative = request.directory.components.join("/");
        let name = request.name.unwrap_or_else(|| {
            request
                .directory
                .components
                .last()
                .cloned()
                .unwrap_or_else(|| grant.name.clone())
        });
        validate_name(&name)?;
        let mut tx = self.store.pool.begin_with("BEGIN IMMEDIATE").await?;
        library_access(&mut tx, actor, library).await?;
        let connected: Vec<Folder> = sqlx::query_as("SELECT * FROM music_folders WHERE library_id = ? AND host_device = ? AND connected = 1")
            .bind(library).bind(self.host.as_str()).fetch_all(&mut *tx).await?;
        for folder in &connected {
            if folder.directory_id.as_deref() == Some(&request.directory.directory_id) {
                if folder.relative_path == relative {
                    let id = folder.id.clone();
                    tx.commit().await?;
                    return self
                        .folder_summary(actor, self.folder_record(&id).await?)
                        .await;
                }
                if overlaps(&folder.relative_path, &relative) {
                    return Err(Error::Conflict(
                        "This directory is covered by another connected music folder".into(),
                    ));
                }
            }
        }
        if connected.len() >= 128 {
            return Err(Error::Invalid(
                "Connect at most 128 music folders per library".into(),
            ));
        }
        let existing: Option<String> = sqlx::query_scalar("SELECT id FROM music_folders WHERE library_id = ? AND host_device = ? AND directory_id = ? AND relative_path = ?")
            .bind(library).bind(self.host.as_str()).bind(&request.directory.directory_id).bind(&relative).fetch_optional(&mut *tx).await?;
        let id = existing.unwrap_or_else(|| Uuid::new_v4().to_string());
        sqlx::query("INSERT INTO music_folders(id, library_id, host_device, directory_id, relative_path, root_path, name, bound) VALUES (?, ?, ?, ?, ?, ?, ?, 1)
            ON CONFLICT(id) DO UPDATE SET connected = 1, bound = 1, name = excluded.name, root_path = excluded.root_path, generation = generation + 1, revision = revision + 1")
            .bind(&id).bind(library).bind(self.host.as_str()).bind(&request.directory.directory_id).bind(relative).bind(grant.root.to_string_lossy().as_ref()).bind(name.trim())
            .execute(&mut *tx).await?;
        let folder: Folder = sqlx::query_as("SELECT * FROM music_folders WHERE id = ?")
            .bind(&id)
            .fetch_one(&mut *tx)
            .await?;
        service::queue(&mut tx, actor, &folder).await?;
        tx.commit().await?;
        drop(root);
        tracing::info!(event = "music_folder_connected", folder_id = %id, "Music folder connected and scan queued");
        self.folder_summary(actor, folder).await
    }

    pub(super) async fn folder_record(&self, id: &str) -> Result<Folder> {
        sqlx::query_as("SELECT * FROM music_folders WHERE id = ? AND host_device = ?")
            .bind(id)
            .bind(self.host.as_str())
            .fetch_optional(&self.store.pool)
            .await?
            .ok_or(Error::NotFound)
    }

    async fn folder_summary(&self, actor: &Actor, folder: Folder) -> Result<MusicFolder> {
        let directory = folder.directory();
        let available = match &directory {
            Some(selection) if folder.bound => {
                self.directories.open_selected(selection).await.is_ok()
            }
            _ => false,
        };
        let latest: Option<String> = sqlx::query_scalar(
            "SELECT id FROM jobs WHERE folder_id = ? ORDER BY rowid DESC LIMIT 1",
        )
        .bind(&folder.id)
        .fetch_optional(&self.store.pool)
        .await?;
        let latest_job = match latest {
            Some(id) => Some(self.job(actor, &id).await?),
            None => None,
        };
        Ok(MusicFolder {
            id: folder.id,
            library_id: folder.library_id,
            host_device: folder.host_device,
            directory,
            name: folder.name,
            revision: folder.revision,
            connected: folder.connected,
            available,
            last_scan_at: folder.last_scan_at,
            latest_job,
        })
    }

    pub(super) async fn ensure_legacy(
        &self,
        tx: &mut Transaction<'_, Sqlite>,
        actor: &Actor,
        library: &str,
    ) -> Result<()> {
        let Some(root) = &self.directories.legacy else {
            return Ok(());
        };
        let exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM music_folders WHERE library_id = ? AND host_device = ? AND directory_id = 'legacy-music')")
            .bind(library).bind(self.host.as_str()).fetch_one(&mut **tx).await?;
        if !exists {
            sqlx::query("INSERT INTO music_folders(id, library_id, host_device, directory_id, root_path, name, bound) VALUES (?, ?, ?, 'legacy-music', ?, 'Music folder', 1)")
                .bind(Uuid::new_v4().to_string()).bind(library).bind(self.host.as_str()).bind(root.to_string_lossy().as_ref()).execute(&mut **tx).await?;
            tracing::info!(
                account_id = actor.id(),
                event = "legacy_music_folder_connected",
                "Configured legacy folder connected"
            );
        }
        Ok(())
    }
}

pub(super) async fn withdraw(tx: &mut Transaction<'_, Sqlite>, folder: &str) -> Result<()> {
    sqlx::query("UPDATE jobs SET state = 'failed', error = 'Music folder changed. Scan the connected folder again.', lease_until = NULL, lease_token = NULL WHERE folder_id = ? AND state IN ('queued','running')")
        .bind(folder).execute(&mut **tx).await?;
    sqlx::query("UPDATE sources SET ready = 0 WHERE id IN (SELECT source_id FROM local_files WHERE folder_id = ?)")
        .bind(folder).execute(&mut **tx).await?;
    metadata_owner::refresh_folder_metadata(tx, folder).await
}

fn validate_name(name: &str) -> Result<()> {
    if name.trim().is_empty() || name.len() > 256 || name.chars().any(char::is_control) {
        return Err(Error::Invalid("Use a folder name of 1 to 256 bytes".into()));
    }
    Ok(())
}

fn overlaps(a: &str, b: &str) -> bool {
    a.is_empty()
        || b.is_empty()
        || a.strip_prefix(b).is_some_and(|rest| rest.starts_with('/'))
        || b.strip_prefix(a).is_some_and(|rest| rest.starts_with('/'))
}
