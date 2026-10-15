mod entries;
mod likes;
mod playlists;
mod progress;

use super::{Action, Operation};
use crate::{
    Actor, Result,
    metadata::{self, RecordingField},
};
type Transaction<'a> = sqlx::Transaction<'a, sqlx::Sqlite>;

#[tracing::instrument(level = "debug", skip_all, err(Debug), fields(account_id = actor.id(), action_kind = operation.action.kind(), operation_id = %operation.id))]
pub(super) async fn action(
    tx: &mut Transaction<'_>,
    actor: &Actor,
    operation: &Operation,
) -> Result<()> {
    match &operation.action {
        Action::RecordingTitleSet {
            recording_id,
            title,
        } => {
            metadata::set_recording_field(
                tx,
                actor,
                recording_id,
                RecordingField::Title,
                Some(title),
                &operation.id,
            )
            .await
        }
        Action::RecordingTitleInherit { recording_id } => {
            metadata::set_recording_field(
                tx,
                actor,
                recording_id,
                RecordingField::Title,
                None,
                &operation.id,
            )
            .await
        }
        Action::RecordingArtistSet {
            recording_id,
            artist,
        } => {
            metadata::set_recording_field(
                tx,
                actor,
                recording_id,
                RecordingField::Artist,
                Some(artist),
                &operation.id,
            )
            .await
        }
        Action::RecordingArtistInherit { recording_id } => {
            metadata::set_recording_field(
                tx,
                actor,
                recording_id,
                RecordingField::Artist,
                None,
                &operation.id,
            )
            .await
        }
        Action::LikeSet {
            recording_id,
            liked,
        } => likes::set(tx, actor, recording_id, *liked).await,
        Action::ProgressSet {
            recording_id,
            position_ms,
            speed,
        } => progress::set(tx, actor, recording_id, *position_ms, *speed).await,
        Action::PlaylistCreate { playlist_id, name } => {
            playlists::create(tx, actor, playlist_id, name).await
        }
        Action::PlaylistRename { playlist_id, name } => {
            playlists::rename(tx, actor, playlist_id, name).await
        }
        Action::PlaylistDelete { playlist_id } => playlists::delete(tx, actor, playlist_id).await,
        Action::EntryInsert {
            playlist_id,
            entry_id,
            recording_id,
            position,
        } => entries::insert(tx, actor, playlist_id, entry_id, recording_id, *position).await,
        Action::EntryRemove {
            playlist_id,
            entry_id,
        } => entries::remove(tx, actor, playlist_id, entry_id).await,
        Action::EntryMove {
            playlist_id,
            entry_id,
            position,
        } => entries::move_entry(tx, actor, playlist_id, entry_id, *position).await,
    }
}
