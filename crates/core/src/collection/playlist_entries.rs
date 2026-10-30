use super::{ImportedPlaylistEntryKind, ImportedPlaylistEntryOrigin, PlaylistEntry};
use crate::{
    Actor, Error, HostDeviceId, Result,
    catalog::{push_available, push_plays_from_services},
    metadata,
};
use sqlx::{QueryBuilder, Row, SqliteConnection, sqlite::SqliteRow};

pub(super) enum EntryAccess {
    All,
    Authorized,
}

pub(super) async fn read(
    connection: &mut SqliteConnection,
    host: &HostDeviceId,
    actor: &Actor,
    playlist: &str,
    access: EntryAccess,
) -> Result<Vec<PlaylistEntry>> {
    let mut query = QueryBuilder::new(
        "SELECT e.id AS entry_id, e.recording_id, e.position,
                COALESCE(k.liked, 0) AS liked, (m.user_id IS NOT NULL) AS authorized,
                COALESCE(imported.kind,followed.kind) AS import_kind,
                COALESCE(imported.provider_recording_id,followed.provider_recording_id) AS provider_recording_id,
                COALESCE(imported.occurrence_id,followed.occurrence_id) AS occurrence_id,
                CASE WHEN e.recording_id IS NULL AND COALESCE(imported.kind,followed.kind) IS NOT NULL THEN COALESCE(imported.title,followed.title,
                    CASE WHEN COALESCE(imported.kind,followed.kind) = 'unknown' THEN 'Unknown provider item' ELSE 'Unavailable provider item' END)
                    WHEN m.user_id IS NULL THEN 'Unavailable recording'
                    ELSE COALESCE(personal_metadata.override_title, r.title) END AS title,
                CASE WHEN e.recording_id IS NULL AND COALESCE(imported.kind,followed.kind) IS NOT NULL THEN COALESCE(imported.title,followed.title,
                    CASE WHEN COALESCE(imported.kind,followed.kind) = 'unknown' THEN 'Unknown provider item' ELSE 'Unavailable provider item' END)
                    WHEN m.user_id IS NULL THEN 'Unavailable recording' ELSE r.title END AS inherited_title,
                CASE WHEN e.recording_id IS NULL AND COALESCE(imported.kind,followed.kind) IS NOT NULL THEN COALESCE(imported.artist,followed.artist, '')
                    WHEN m.user_id IS NULL THEN '' ELSE COALESCE(personal_metadata.override_artist, r.artist) END AS artist,
                CASE WHEN e.recording_id IS NULL AND COALESCE(imported.kind,followed.kind) IS NOT NULL THEN COALESCE(imported.artist,followed.artist, '')
                    WHEN m.user_id IS NULL THEN '' ELSE r.artist END AS inherited_artist,
                CASE WHEN m.user_id IS NULL THEN 0 ELSE r.duration_ms END AS duration_ms,
                (m.user_id IS NOT NULL AND ",
    );
    push_available(&mut query, host, actor.id());
    query.push(") AS available, (m.user_id IS NOT NULL AND ");
    push_plays_from_services(&mut query, actor.id());
    query.push(
        ") AS plays_from_services FROM playlist_entries e
         LEFT JOIN playlist_import_entries imported ON imported.playlist_id = e.playlist_id AND imported.entry_id = e.id
         LEFT JOIN playlist_follow_entries followed ON followed.playlist_id = e.playlist_id AND followed.entry_id = e.id
         LEFT JOIN recordings r ON r.id = e.recording_id
         LEFT JOIN library_members m ON m.library_id = r.library_id AND m.user_id = ",
    ).push_bind(actor.id().to_owned()).push(
        " LEFT JOIN likes k ON k.recording_id = r.id AND k.user_id = m.user_id ",
    );
    metadata::join_recording_fields(&mut query, actor);
    query
        .push(" WHERE e.playlist_id = ")
        .push_bind(playlist.to_owned())
        .push(" AND e.deleted = 0");
    if matches!(access, EntryAccess::Authorized) {
        query.push(" AND m.user_id IS NOT NULL");
    }
    query.push(" ORDER BY e.position, e.id LIMIT 10000");
    query
        .build()
        .fetch_all(connection)
        .await?
        .into_iter()
        .map(entry)
        .collect()
}

fn entry(row: SqliteRow) -> Result<PlaylistEntry> {
    let recording_id: Option<String> = row.try_get("recording_id")?;
    let authorized: bool = row.try_get("authorized")?;
    let kind: Option<String> = row.try_get("import_kind")?;
    let origin = if recording_id.is_none() || authorized {
        kind.map(|kind| {
            let kind = match kind.as_str() {
                "recording" => ImportedPlaylistEntryKind::Recording,
                "unavailable" => ImportedPlaylistEntryKind::Unavailable,
                "unknown" => ImportedPlaylistEntryKind::Unknown,
                _ => {
                    return Err(Error::Internal(
                        "Stored playlist entry kind is invalid".into(),
                    ));
                }
            };
            Ok(ImportedPlaylistEntryOrigin {
                kind,
                provider_recording_id: row.try_get("provider_recording_id")?,
                occurrence_id: row.try_get("occurrence_id")?,
            })
        })
        .transpose()?
    } else {
        None
    };
    Ok(PlaylistEntry {
        entry_id: row.try_get("entry_id")?,
        liked: row.try_get("liked")?,
        recording_id,
        position: row.try_get("position")?,
        title: row.try_get("title")?,
        inherited_title: row.try_get("inherited_title")?,
        inherited_artist: row.try_get("inherited_artist")?,
        artist: row.try_get("artist")?,
        duration_ms: row.try_get("duration_ms")?,
        available: row.try_get("available")?,
        plays_from_services: row.try_get("plays_from_services")?,
        origin,
    })
}
