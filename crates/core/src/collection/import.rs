use super::{
    Collection, OpenedProviderPlaylist, PlaylistImportReceipt, import_persist, import_receipts,
};
use crate::{Actor, Error, Result};
use hito_plugin_sdk::{Capability, PlaylistDetailRequest};
use serde::{Deserialize, Serialize};
use sqlx::Connection;
use utoipa::ToSchema;
use uuid::Uuid;

#[derive(Deserialize, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ImportProviderPlaylist {
    pub request_id: Uuid,
    pub installation_revision: i64,
    pub playlist_reference: String,
}

impl Collection {
    #[tracing::instrument(level = "info", skip_all, err(Debug), fields(account_id = actor.id(), request_id = %request.request_id))]
    pub async fn import_provider_playlist(
        &self,
        actor: &Actor,
        installation: &str,
        request: ImportProviderPlaylist,
    ) -> Result<PlaylistImportReceipt> {
        let opened = self
            .import_or_reuse(actor, installation, request, false)
            .await?;
        Ok(PlaylistImportReceipt {
            request_id: opened.request_id,
            playlist_id: opened.playlist_id,
        })
    }

    #[tracing::instrument(level = "info", skip_all, err(Debug), fields(account_id = actor.id(), request_id = %request.request_id))]
    pub async fn open_provider_playlist(
        &self,
        actor: &Actor,
        installation: &str,
        request: ImportProviderPlaylist,
    ) -> Result<OpenedProviderPlaylist> {
        self.import_or_reuse(actor, installation, request, true)
            .await
    }

    async fn import_or_reuse(
        &self,
        actor: &Actor,
        installation: &str,
        request: ImportProviderPlaylist,
        reuse: bool,
    ) -> Result<OpenedProviderPlaylist> {
        let started = std::time::Instant::now();
        PlaylistDetailRequest::Reference {
            reference: request.playlist_reference.clone(),
        }
        .validate()
        .map_err(|message| Error::Invalid(message.into()))?;
        if request.installation_revision <= 0 {
            return Err(Error::Invalid(
                "Use a positive plugin connection revision".into(),
            ));
        }
        let digest = import_receipts::fingerprint(installation, &request)?;
        let mut connection = self.store.pool.acquire().await?;
        actor.check_session_on(&mut connection).await?;
        let previous =
            import_receipts::lookup(&mut connection, actor.id(), request.request_id, &digest)
                .await?;
        actor.check_session_on(&mut connection).await?;
        drop(connection);
        if let Some(receipt) = previous {
            tracing::info!(
                outcome = "replayed",
                elapsed_ms = started.elapsed().as_millis() as u64,
                "Playlist copy completed"
            );
            return Ok(opened(receipt, false));
        }
        let operation = self
            .plugins
            .operation(
                actor,
                installation,
                request.installation_revision,
                &[Capability::Playlists],
            )
            .await?;
        let snapshot = operation
            .playlist_snapshot(request.playlist_reference.clone())
            .await?;
        let mut connection = self.store.pool.acquire().await?;
        tracing::debug!(
            event = "playlist_import_write_started",
            "Publishing provider playlist copy"
        );
        let mut tx = connection.begin_with("BEGIN IMMEDIATE").await?;
        actor.check_session_on(&mut tx).await?;
        if let Some(receipt) =
            import_receipts::lookup(&mut tx, actor.id(), request.request_id, &digest).await?
        {
            tx.commit().await?;
            tracing::info!(
                outcome = "replayed",
                elapsed_ms = started.elapsed().as_millis() as u64,
                "Playlist copy completed"
            );
            return Ok(opened(receipt, false));
        }
        operation.recheck(&mut tx).await?;
        if reuse
            && let Some(existing) =
                existing_copy(&mut tx, actor, installation, &snapshot.detail.id).await?
        {
            tx.commit().await?;
            tracing::info!(
                outcome = "reused",
                elapsed_ms = started.elapsed().as_millis() as u64,
                "Playlist copy completed"
            );
            return Ok(OpenedProviderPlaylist {
                request_id: request.request_id,
                playlist_id: existing,
                reused: true,
            });
        }
        let entry_count = snapshot.entries.len();
        let receipt = import_persist::publish(
            &mut tx,
            actor,
            installation,
            &request,
            &operation,
            snapshot,
            &digest,
        )
        .await?;
        tx.commit().await?;
        self.changes
            .playlist_changed(actor.id(), receipt.playlist_id.to_string());
        tracing::info!(
            outcome = "imported",
            entries = entry_count,
            elapsed_ms = started.elapsed().as_millis() as u64,
            "Playlist copy completed"
        );
        Ok(opened(receipt, false))
    }
}

fn opened(receipt: PlaylistImportReceipt, reused: bool) -> OpenedProviderPlaylist {
    OpenedProviderPlaylist {
        request_id: receipt.request_id,
        playlist_id: receipt.playlist_id,
        reused,
    }
}

pub(crate) async fn existing_copy(
    connection: &mut sqlx::SqliteConnection,
    actor: &Actor,
    installation: &str,
    provider_playlist: &str,
) -> Result<Option<Uuid>> {
    let found: Option<String> = sqlx::query_scalar("SELECT playlist.id FROM playlist_imports origin JOIN playlists playlist ON playlist.id = origin.playlist_id WHERE playlist.user_id = ? AND playlist.deleted = 0 AND playlist.kind = 'local' AND origin.installation_id = ? AND origin.provider_playlist_id = ? ORDER BY origin.imported_at DESC, playlist.id LIMIT 1")
        .bind(actor.id())
        .bind(installation)
        .bind(provider_playlist)
        .fetch_optional(connection)
        .await?;
    found
        .map(|id| Uuid::parse_str(&id))
        .transpose()
        .map_err(|_| Error::Internal("Stored playlist identity is invalid".into()))
}
