use crate::collection::{ImportedPlaylistOrigin, import_persist};
use crate::{Actor, Error, Result, catalog::ExistingProviderMetadata, plugins::PlaylistOperation};
use sqlx::SqliteConnection;
use uuid::Uuid;

pub(super) struct Scope {
    pub origin: ImportedPlaylistOrigin,
    pub targets: Vec<ExistingProviderMetadata>,
}

pub(super) fn identity(value: &str) -> Result<Uuid> {
    Uuid::parse_str(value)
        .ok()
        .filter(|id| !id.is_nil())
        .ok_or(Error::NotFound)
}

pub(super) async fn copy_origin(
    connection: &mut SqliteConnection,
    actor: &Actor,
    playlist: &str,
) -> Result<ImportedPlaylistOrigin> {
    let accessible: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM playlists WHERE id = ? AND user_id = ? AND kind = 'local' AND deleted = 0)",
    ).bind(playlist).bind(actor.id()).fetch_one(&mut *connection).await?;
    if !accessible {
        return Err(Error::NotFound);
    }
    import_persist::origin(connection, playlist)
        .await?
        .ok_or(Error::NotFound)
}

pub(super) async fn recheck_origin(
    connection: &mut SqliteConnection,
    actor: &Actor,
    playlist: &str,
    expected: &ImportedPlaylistOrigin,
) -> Result<()> {
    let current = copy_origin(connection, actor, playlist).await?;
    let encode = |origin: &ImportedPlaylistOrigin| {
        serde_json::to_vec(origin)
            .map_err(|_| Error::Internal("Playlist origin could not be encoded".into()))
    };
    if encode(&current)? != encode(expected)? {
        return Err(Error::Conflict(
            "The playlist origin changed. Review the copy again.".into(),
        ));
    }
    Ok(())
}

pub(super) fn check_operation(
    origin: &ImportedPlaylistOrigin,
    operation: &PlaylistOperation,
) -> Result<()> {
    if operation.installation != origin.installation_id
        || operation.service != origin.service_id
        || operation.provider != origin.provider
    {
        return Err(Error::Conflict(
            "The reviewed connection does not match this playlist origin".into(),
        ));
    }
    Ok(())
}

pub(super) async fn targets(
    connection: &mut SqliteConnection,
    actor: &Actor,
    playlist: &str,
    origin: &ImportedPlaylistOrigin,
) -> Result<Vec<ExistingProviderMetadata>> {
    let targets: Vec<ExistingProviderMetadata> = sqlx::query_as(
        "SELECT DISTINCT recording.id AS recording_id,recording.library_id,
            access.id AS access_grant,binding.id AS binding_id,binding.revision AS binding_revision,
            binding.provider_recording_id,observation.id AS observation_id
         FROM playlist_entries entry
         JOIN playlist_import_entries imported ON imported.playlist_id = entry.playlist_id
            AND imported.entry_id = entry.id AND imported.kind = 'recording'
         JOIN recordings recording ON recording.id = entry.recording_id AND recording.release_id IS NULL
         JOIN libraries library ON library.id = recording.library_id AND library.owner_id = ?
         JOIN library_access_grants access ON access.library_id = library.id AND access.user_id = ?
         JOIN provider_items binding ON binding.id = recording.metadata_binding_id
            AND binding.recording_id = recording.id AND binding.installation_id = ?
            AND binding.provider_recording_id = imported.provider_recording_id
            AND binding.role = 'primary' AND binding.retired = 0
         JOIN metadata_observations observation ON observation.id = recording.metadata_observation_id
            AND observation.recording_id = recording.id AND observation.provider_binding_id = binding.id
            AND observation.plugin_installation_id = binding.installation_id
            AND observation.provider_recording_id = binding.provider_recording_id
            AND observation.origin = 'provider_response' AND observation.provider = ?
         WHERE entry.playlist_id = ? AND entry.deleted = 0 ORDER BY recording.id LIMIT 10001",
    ).bind(actor.id()).bind(actor.id()).bind(&origin.installation_id).bind(&origin.provider)
        .bind(playlist).fetch_all(connection).await?;
    if targets.len() > hito_plugin_sdk::MAX_PLAYLIST_ITEMS {
        return Err(Error::Invalid(
            "This playlist has too many metadata targets".into(),
        ));
    }
    Ok(targets)
}
