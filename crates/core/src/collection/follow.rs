mod approval;
pub(in crate::collection) mod history;
mod jobs;
mod lease;
mod persist;
mod receipts;
mod reconcile;
mod resume;
mod types;
mod worker;
pub(super) use persist::origin;
pub(super) use resume::resume;

pub use types::{
    FollowProviderPlaylist, PlaylistFollow, PlaylistFollowInterval, PlaylistFollowReceipt,
    PlaylistFollowRefresh, PlaylistFollowStatus, RefreshFollowedPlaylist,
};

use super::Collection;
use crate::{Actor, Error, Result};
use hito_plugin_sdk::PlaylistDetailRequest;
use sqlx::Connection;

impl Collection {
    #[tracing::instrument(level = "info", skip_all, err(Debug), fields(account_id = actor.id(), request_id = %request.request_id))]
    pub async fn follow_provider_playlist(
        &self,
        actor: &Actor,
        installation: &str,
        request: FollowProviderPlaylist,
    ) -> Result<PlaylistFollowReceipt> {
        let started = std::time::Instant::now();
        let reference = PlaylistDetailRequest::Reference {
            reference: request.playlist_reference.clone(),
        };
        reference
            .validate()
            .map_err(|message| Error::Invalid(message.into()))?;
        if request.installation_revision <= 0 {
            return Err(Error::Invalid(
                "Use a positive plugin connection revision".into(),
            ));
        }
        let digest = receipts::fingerprint(installation, &request)?;
        let mut connection = self.store.pool.acquire().await?;
        actor.check_session_on(&mut connection).await?;
        let previous =
            receipts::lookup(&mut connection, actor.id(), request.request_id, &digest).await?;
        actor.check_session_on(&mut connection).await?;
        drop(connection);
        if let Some(receipt) = previous {
            tracing::info!(
                outcome = "replayed",
                elapsed_ms = started.elapsed().as_millis() as u64,
                "Playlist follow completed"
            );
            return Ok(receipt);
        }
        let operation = self
            .plugins
            .playlist_operation(actor.id(), installation, request.installation_revision)
            .await?;
        let snapshot = operation
            .snapshot(reference, || async {
                let mut connection = self.store.pool.acquire().await?;
                actor.check_session_on(&mut connection).await?;
                operation.recheck(&mut connection).await
            })
            .await?;
        let mut connection = self.store.pool.acquire().await?;
        let mut tx = connection.begin_with("BEGIN IMMEDIATE").await?;
        actor.check_session_on(&mut tx).await?;
        if let Some(receipt) =
            receipts::lookup(&mut tx, actor.id(), request.request_id, &digest).await?
        {
            tx.commit().await?;
            return Ok(receipt);
        }
        operation.recheck(&mut tx).await?;
        let count: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM playlist_follows WHERE user_id = ? AND active = 1",
        )
        .bind(actor.id())
        .fetch_one(&mut *tx)
        .await?;
        let total: i64 =
            sqlx::query_scalar("SELECT count(*) FROM playlist_follows WHERE active = 1")
                .fetch_one(&mut *tx)
                .await?;
        if count >= 256 || total >= 4096 {
            return Err(Error::Conflict(
                "The followed playlist limit has been reached".into(),
            ));
        }
        let entries = snapshot.entries.len();
        let receipt = persist::create(&mut tx, &operation, &request, snapshot, &digest).await?;
        actor.check_session_on(&mut tx).await?;
        tx.commit().await?;
        tracing::info!(
            outcome = "followed",
            entries,
            elapsed_ms = started.elapsed().as_millis() as u64,
            "Playlist follow completed"
        );
        Ok(receipt)
    }
}

impl PlaylistFollowInterval {
    pub(super) fn seconds(self) -> i64 {
        match self {
            Self::Hourly => 3600,
            Self::Daily => 86400,
        }
    }

    fn from_seconds(seconds: i64) -> Result<Self> {
        match seconds {
            3600 => Ok(Self::Hourly),
            86400 => Ok(Self::Daily),
            _ => Err(Error::Internal(
                "Stored playlist refresh interval is invalid".into(),
            )),
        }
    }
}
