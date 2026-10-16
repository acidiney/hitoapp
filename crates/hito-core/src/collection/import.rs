use super::{Collection, PlaylistImportReceipt, import_persist, import_receipts};
use crate::{Actor, Error, Result};
use hito_plugin_sdk::{Capability, PlaylistDetailRequest};
use serde::{Deserialize, Serialize};
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
            return Ok(receipt);
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
        let mut tx = self.store.pool.begin_with("BEGIN IMMEDIATE").await?;
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
            return Ok(receipt);
        }
        operation.recheck(&mut tx).await?;
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
        tracing::info!(
            outcome = "imported",
            entries = entry_count,
            elapsed_ms = started.elapsed().as_millis() as u64,
            "Playlist copy completed"
        );
        Ok(receipt)
    }
}
