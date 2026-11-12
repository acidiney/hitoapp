mod authority;
mod publication;
mod receipts;

use super::Collection;
use crate::{Actor, Error, Result, admission::Accounts, plugins::PlaylistOperationFailure};
use hito_plugin_sdk::PlaylistDetailRequest;
use serde::{Deserialize, Serialize};
use std::{sync::Arc, time::Instant};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use utoipa::ToSchema;
use uuid::Uuid;

#[derive(Deserialize, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct RefreshImportedPlaylistMetadata {
    pub request_id: Uuid,
    pub installation_revision: i64,
}

#[derive(Serialize, ToSchema)]
pub struct ImportedPlaylistMetadataRefresh {
    pub request_id: Uuid,
    pub playlist_id: Uuid,
    pub updated_recordings: u32,
    pub unchanged_recordings: u32,
    pub unavailable_recordings: u32,
}

#[derive(Clone)]
pub(crate) struct Admission {
    accounts: Accounts,
    slots: Arc<Semaphore>,
}

impl Default for Admission {
    fn default() -> Self {
        Self {
            accounts: Accounts::default(),
            slots: Arc::new(Semaphore::new(4)),
        }
    }
}

impl Admission {
    fn enter(&self, account: &str) -> Result<(OwnedSemaphorePermit, OwnedSemaphorePermit)> {
        let account = self.accounts.enter(account)?;
        let global = self.slots.clone().try_acquire_owned().map_err(|_| {
            Error::Conflict("Playlist metadata requests are busy. Try again.".into())
        })?;
        Ok((account, global))
    }
}

impl Collection {
    #[tracing::instrument(level = "info", skip_all, err(Debug), fields(account_id = actor.id(), request_id = %request.request_id))]
    pub async fn refresh_imported_playlist_metadata(
        &self,
        actor: &Actor,
        playlist: &str,
        request: RefreshImportedPlaylistMetadata,
    ) -> Result<ImportedPlaylistMetadataRefresh> {
        let started = Instant::now();
        let playlist_id = authority::identity(playlist)?;
        let playlist = playlist_id.to_string();
        if request.request_id.is_nil() || request.installation_revision <= 0 {
            return Err(Error::Invalid(
                "Use a nonempty request ID and positive connection revision".into(),
            ));
        }
        let digest = receipts::fingerprint(&playlist, &request)?;
        let origin = {
            let mut tx = self.store.pool.begin().await?;
            actor.check_session_on(&mut tx).await?;
            if let Some(receipt) =
                receipts::lookup(&mut tx, actor.id(), request.request_id, &digest).await?
            {
                actor.check_session_on(&mut tx).await?;
                tx.commit().await?;
                tracing::info!(
                    outcome = "replayed",
                    elapsed_ms = started.elapsed().as_millis() as u64,
                    "Playlist metadata request completed"
                );
                return Ok(receipt);
            }
            let origin = authority::copy_origin(&mut tx, actor, &playlist).await?;
            tx.commit().await?;
            origin
        };
        let _admission = self.metadata_refresh_admission.enter(actor.id())?;
        let operation = match self
            .plugins
            .playlist_operation(
                actor.id(),
                &origin.installation_id,
                request.installation_revision,
            )
            .await
        {
            Ok(operation) => operation,
            Err(PlaylistOperationFailure::RevisionChanged { expected, current })
                if current > expected =>
            {
                let mut tx = self.store.pool.begin().await?;
                actor.check_session_on(&mut tx).await?;
                let accepted =
                    receipts::lookup(&mut tx, actor.id(), request.request_id, &digest).await?;
                actor.check_session_on(&mut tx).await?;
                tx.commit().await?;
                if let Some(receipt) = accepted {
                    tracing::info!(
                        outcome = "replayed",
                        elapsed_ms = started.elapsed().as_millis() as u64,
                        "Playlist metadata request completed"
                    );
                    return Ok(receipt);
                }
                tracing::info!(
                    outcome = "rejected",
                    reason = "connection_revision_advanced",
                    elapsed_ms = started.elapsed().as_millis() as u64,
                    "Playlist metadata request completed"
                );
                return Err(Error::OperationRejected(
                    "This playlist connection changed".into(),
                ));
            }
            Err(error) => return Err(error.into()),
        };
        authority::check_operation(&origin, &operation)?;
        let scope = {
            let mut tx = self.store.pool.begin().await?;
            actor.check_session_on(&mut tx).await?;
            operation.recheck(&mut tx).await?;
            authority::recheck_origin(&mut tx, actor, &playlist, &origin).await?;
            receipts::capacity(&mut tx, actor.id()).await?;
            let targets = authority::targets(&mut tx, actor, &playlist, &origin).await?;
            tx.commit().await?;
            authority::Scope { origin, targets }
        };
        let snapshot = operation
            .snapshot(
                PlaylistDetailRequest::Canonical {
                    playlist_id: scope.origin.provider_playlist_id.clone(),
                },
                || async {
                    let mut connection = self.store.pool.acquire().await?;
                    actor.check_session_on(&mut connection).await?;
                    operation.recheck(&mut connection).await?;
                    authority::recheck_origin(&mut connection, actor, &playlist, &scope.origin)
                        .await
                },
            )
            .await?;
        if snapshot.detail.id != scope.origin.provider_playlist_id {
            return Err(Error::Upstream(
                "The provider returned a different playlist",
            ));
        }
        tracing::debug!(
            event = "playlist_metadata_write_started",
            "Publishing verified playlist metadata"
        );
        let mut tx = self.store.pool.begin_with("BEGIN IMMEDIATE").await?;
        actor.check_session_on(&mut tx).await?;
        if let Some(receipt) =
            receipts::lookup(&mut tx, actor.id(), request.request_id, &digest).await?
        {
            actor.check_session_on(&mut tx).await?;
            tx.commit().await?;
            tracing::info!(
                outcome = "replayed",
                elapsed_ms = started.elapsed().as_millis() as u64,
                "Playlist metadata request completed"
            );
            return Ok(receipt);
        }
        authority::recheck_origin(&mut tx, actor, &playlist, &scope.origin).await?;
        operation.recheck(&mut tx).await?;
        let current = authority::targets(&mut tx, actor, &playlist, &scope.origin).await?;
        if current != scope.targets {
            return Err(Error::Conflict(
                "The playlist metadata owners changed. Review the copy again.".into(),
            ));
        }
        receipts::capacity(&mut tx, actor.id()).await?;
        let (receipt, sequence) = publication::publish(
            &mut tx,
            actor,
            playlist_id,
            request.request_id,
            &scope,
            &operation,
            snapshot,
        )
        .await?;
        receipts::save(&mut tx, actor.id(), &digest, &receipt, sequence).await?;
        actor.check_session_on(&mut tx).await?;
        tx.commit().await?;
        self.changes.playlist_changed(actor.id(), playlist.as_str());
        self.changes
            .publish(actor.id(), [crate::ChangeNotice::Recordings]);
        tracing::info!(
            outcome = "complete",
            updated = receipt.updated_recordings,
            unchanged = receipt.unchanged_recordings,
            unavailable = receipt.unavailable_recordings,
            elapsed_ms = started.elapsed().as_millis() as u64,
            "Playlist metadata request completed"
        );
        Ok(receipt)
    }
}
