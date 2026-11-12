use super::{Action, Collection, SyncBatch, SyncReceipt, apply};
use crate::{Actor, ChangeNotice, Error, Result};

impl Collection {
    #[tracing::instrument(level = "info", skip_all, err(Debug), fields(account_id = actor.id(), operation_count = batch.operations.len()))]
    pub async fn apply(&self, actor: &Actor, batch: SyncBatch) -> Result<SyncReceipt> {
        validate_batch(&batch)?;
        let mut tx = self.store.pool.begin_with("BEGIN IMMEDIATE").await?;
        let mut notices = Vec::new();
        match apply_batch(&mut tx, actor, batch, &mut notices).await {
            Ok(receipt) => {
                tx.commit().await?;
                tracing::info!(
                    accepted = receipt.acknowledged.len(),
                    "Sync batch committed"
                );
                self.changes.publish(actor.id(), notices);
                Ok(receipt)
            }
            Err(error) => {
                tx.rollback().await?;
                tracing::warn!(error_code = error.code(), "Sync batch rolled back");
                Err(error)
            }
        }
    }
}

async fn apply_batch(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    actor: &Actor,
    batch: SyncBatch,
    notices: &mut Vec<ChangeNotice>,
) -> Result<SyncReceipt> {
    actor.check_session_on(tx).await?;
    let mut acknowledged = Vec::with_capacity(batch.operations.len());
    for operation in batch.operations {
        let id = operation.id.to_string();
        let payload =
            serde_json::to_string(&operation).map_err(|e| Error::Internal(e.to_string()))?;
        if payload.len() > super::events::MAX_SYNC_PAYLOAD_BYTES {
            return Err(Error::Invalid(
                "The sync operation exceeds the size limit".into(),
            ));
        }
        let previous: Option<(String, String)> = sqlx::query_as(
            "SELECT device_id, payload FROM sync_events
                 WHERE user_id = ? AND origin = 'client_operation' AND event_id = ?",
        )
        .bind(actor.id())
        .bind(&id)
        .fetch_optional(&mut **tx)
        .await?;
        if let Some((device, previous)) = previous {
            if previous != payload || device != batch.device_id {
                tracing::warn!(
                    action_kind = operation.action.kind(),
                    "Sync operation ID conflict"
                );
                return Err(Error::Conflict(
                    "An operation ID was reused with different contents".into(),
                ));
            }
            tracing::debug!(
                action_kind = operation.action.kind(),
                "Sync operation replayed"
            );
            acknowledged.push(operation.id);
            continue;
        }
        apply::action(tx, actor, &operation).await?;
        for notice in changed(&operation.action) {
            if !notices.contains(&notice) {
                notices.push(notice);
            }
        }
        sqlx::query(
            "INSERT INTO sync_events(user_id, origin, device_id, event_id, payload)
                 VALUES (?, 'client_operation', ?, ?, ?)",
        )
        .bind(actor.id())
        .bind(&batch.device_id)
        .bind(id)
        .bind(payload)
        .execute(&mut **tx)
        .await?;
        tracing::debug!(
            action_kind = operation.action.kind(),
            "Sync operation accepted"
        );
        acknowledged.push(operation.id);
    }
    Ok(SyncReceipt { acknowledged })
}

fn changed(action: &Action) -> Vec<ChangeNotice> {
    let recording = |id: &uuid::Uuid| ChangeNotice::Recording {
        recording_id: id.to_string(),
    };
    let playlist = |id: &uuid::Uuid| ChangeNotice::Playlist {
        playlist_id: id.to_string(),
    };
    match action {
        Action::ReleaseTitleSet { .. }
        | Action::ReleaseArtistSet { .. }
        | Action::ReleaseTitleInherit { .. }
        | Action::ReleaseArtistInherit { .. } => vec![ChangeNotice::Recordings],
        Action::RecordingTitleSet { recording_id, .. }
        | Action::RecordingTitleInherit { recording_id }
        | Action::RecordingArtistSet { recording_id, .. }
        | Action::RecordingArtistInherit { recording_id }
        | Action::LikeSet { recording_id, .. } => vec![recording(recording_id)],
        Action::PlaylistCreate { playlist_id, .. }
        | Action::PlaylistRename { playlist_id, .. }
        | Action::PlaylistDelete { playlist_id }
        | Action::EntryInsert { playlist_id, .. }
        | Action::EntryRemove { playlist_id, .. }
        | Action::EntryMove { playlist_id, .. } => {
            vec![playlist(playlist_id), ChangeNotice::Playlists]
        }
        Action::PlaylistFollowResume { playlist_id, .. } => vec![playlist(playlist_id)],
        Action::ProgressSet { .. }
        | Action::AudiobookCreate { .. }
        | Action::AudiobookChapterSet { .. }
        | Action::AudiobookBookmarkCreate { .. }
        | Action::AudiobookBookmarkRemove { .. } => Vec::new(),
    }
}

fn validate_batch(batch: &SyncBatch) -> Result<()> {
    if batch.operations.len() > 100
        || batch.device_id.is_empty()
        || batch.device_id.len() > 128
        || !batch
            .device_id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "-_".contains(c))
    {
        return Err(Error::Invalid("Use a device ID of 1 to 128 letters, numbers, hyphens, or underscores, and at most 100 operations per request".into()));
    }
    Ok(())
}
