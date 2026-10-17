use super::{
    FollowProviderPlaylist, PlaylistFollowInterval, PlaylistFollowReceipt, receipts, reconcile,
};
use crate::collection::{
    CollectionPublication, ImportedPlaylistEntry, ImportedPlaylistEntryKind,
    ImportedPlaylistEntryOrigin, ImportedPlaylistOrigin, ImportedPlaylistRecording,
    ImportedPlaylistSnapshot, import_publication,
};
use crate::{
    Error, Result, catalog,
    plugins::{PlaylistOperation, PlaylistSnapshot},
};
use hito_plugin_sdk::ProviderPlaylistItem;
use sqlx::{Row, Sqlite, SqliteConnection, Transaction};
use std::collections::HashMap;
use uuid::Uuid;

pub(super) async fn create(
    tx: &mut Transaction<'_, Sqlite>,
    operation: &PlaylistOperation,
    request: &FollowProviderPlaylist,
    snapshot: PlaylistSnapshot,
    digest: &str,
) -> Result<PlaylistFollowReceipt> {
    let playlist_id = Uuid::new_v4();
    let playlist = playlist_id.to_string();
    let attribution = serde_json::to_string(&snapshot.detail.attribution)
        .map_err(|_| Error::Internal("Playlist attribution could not be encoded".into()))?;
    sqlx::query("INSERT INTO playlists(id,user_id,name,kind) VALUES (?,?,?,'followed')")
        .bind(&playlist)
        .bind(&operation.account)
        .bind(&snapshot.detail.name)
        .execute(&mut **tx)
        .await?;
    sqlx::query("INSERT INTO playlist_follows(playlist_id,user_id,installation_id,installation_revision,service_id,service_revision,snapshot_installation_revision,snapshot_service_revision,provider,provider_playlist_id,snapshot,content_digest,attribution,interval_seconds,next_refresh_at)
        VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?,unixepoch() + ?)")
        .bind(&playlist).bind(&operation.account).bind(&operation.installation).bind(operation.installation_revision)
        .bind(&operation.service).bind(operation.service_revision)
        .bind(operation.installation_revision).bind(operation.service_revision).bind(&operation.provider)
        .bind(&snapshot.detail.id).bind(&snapshot.detail.snapshot).bind(&snapshot.content_digest).bind(attribution)
        .bind(request.interval.seconds()).bind(request.interval.seconds()).execute(&mut **tx).await?;
    let publication = snapshot_for_publication(tx, operation, &playlist, snapshot).await?;
    let sequence = publish(
        tx,
        &operation.account,
        CollectionPublication::PlaylistFollowed {
            publication_id: Uuid::new_v4(),
            interval: request.interval,
            snapshot: publication,
        },
    )
    .await?;
    sqlx::query("INSERT INTO playlist_follow_receipts(user_id,request_id,request_digest,playlist_id,publication_sequence) VALUES (?,?,?,?,?)")
        .bind(&operation.account).bind(request.request_id.to_string()).bind(digest).bind(&playlist).bind(sequence).execute(&mut **tx).await?;
    Ok(PlaylistFollowReceipt {
        request_id: request.request_id,
        playlist_id,
    })
}

pub(super) async fn refresh(
    tx: &mut Transaction<'_, Sqlite>,
    operation: &PlaylistOperation,
    playlist: &str,
    snapshot: PlaylistSnapshot,
) -> Result<()> {
    let (interval, previous_digest): (i64, String) = sqlx::query_as(
        "SELECT interval_seconds,content_digest FROM playlist_follows WHERE playlist_id = ?",
    )
    .bind(playlist)
    .fetch_one(&mut **tx)
    .await?;
    if previous_digest == snapshot.content_digest {
        sqlx::query("UPDATE playlist_follows SET last_refreshed_at = unixepoch(),next_refresh_at = unixepoch() + interval_seconds,error = NULL WHERE playlist_id = ?")
            .bind(playlist).execute(&mut **tx).await?;
        tracing::debug!(
            outcome = "unchanged",
            "Follow snapshot checked without collection publication"
        );
        return Ok(());
    }
    let attribution = serde_json::to_string(&snapshot.detail.attribution)
        .map_err(|_| Error::Internal("Playlist attribution could not be encoded".into()))?;
    sqlx::query("UPDATE playlist_follows SET snapshot = ?,content_digest = ?,attribution = ?,last_refreshed_at = unixepoch(),next_refresh_at = unixepoch() + interval_seconds,error = NULL WHERE playlist_id = ?")
        .bind(&snapshot.detail.snapshot).bind(&snapshot.content_digest).bind(attribution).bind(playlist).execute(&mut **tx).await?;
    sqlx::query("UPDATE playlists SET name = ? WHERE id = ?")
        .bind(&snapshot.detail.name)
        .bind(playlist)
        .execute(&mut **tx)
        .await?;
    let publication = snapshot_for_publication(tx, operation, playlist, snapshot).await?;
    publish(
        tx,
        &operation.account,
        CollectionPublication::PlaylistFollowRefreshed {
            publication_id: Uuid::new_v4(),
            interval: PlaylistFollowInterval::from_seconds(interval)?,
            snapshot: publication,
        },
    )
    .await?;
    Ok(())
}

async fn snapshot_for_publication(
    tx: &mut Transaction<'_, Sqlite>,
    operation: &PlaylistOperation,
    playlist: &str,
    snapshot: PlaylistSnapshot,
) -> Result<ImportedPlaylistSnapshot> {
    let library = catalog::provider_library(tx, &operation.account).await?;
    let library_id = receipts::identity(&library)?;
    let entry_ids = reconcile::identities(tx, playlist, &snapshot.entries).await?;
    sqlx::query("UPDATE playlist_entries SET deleted = 1 WHERE playlist_id = ?")
        .bind(playlist)
        .execute(&mut **tx)
        .await?;
    let mut identities = HashMap::new();
    let mut recordings = Vec::new();
    let mut entries = Vec::with_capacity(snapshot.entries.len());
    for (position, (entry, entry_id)) in snapshot.entries.into_iter().zip(entry_ids).enumerate() {
        let (recording_id, kind, provider_recording_id, title, artist) = match entry.item {
            ProviderPlaylistItem::Recording {
                recording: metadata,
            } => {
                let remote = metadata.id.clone();
                let recording_id = if let Some(id) = identities.get(&remote) {
                    *id
                } else {
                    let published = catalog::publish_provider_metadata(
                        tx,
                        &library,
                        &operation.installation,
                        &operation.provider,
                        &metadata,
                    )
                    .await?;
                    crate::metadata::refresh_recording_credits(tx, &published.recording_id).await?;
                    if let Some(release) = &published.release_id {
                        crate::metadata::refresh_release(tx, &operation.account, release).await?;
                    }
                    let id = receipts::identity(&published.recording_id)?;
                    identities.insert(remote.clone(), id);
                    recordings.push(ImportedPlaylistRecording {
                        recording_id: id,
                        library_id,
                        release_id: published
                            .release_id
                            .as_deref()
                            .map(receipts::identity)
                            .transpose()?,
                        observation_id: receipts::identity(&published.observation_id)?,
                        metadata,
                    });
                    id
                };
                (
                    Some(recording_id),
                    ImportedPlaylistEntryKind::Recording,
                    Some(remote),
                    None,
                    None,
                )
            }
            ProviderPlaylistItem::Unavailable {
                provider_recording_id,
                title,
                artist,
            } => (
                None,
                ImportedPlaylistEntryKind::Unavailable,
                provider_recording_id,
                title,
                artist,
            ),
            ProviderPlaylistItem::Unknown {} => {
                (None, ImportedPlaylistEntryKind::Unknown, None, None, None)
            }
        };
        sqlx::query("INSERT INTO playlist_entries(playlist_id,id,recording_id,position) VALUES (?,?,?,?)
            ON CONFLICT(playlist_id,id) DO UPDATE SET recording_id = excluded.recording_id,position = excluded.position,deleted = 0")
            .bind(playlist).bind(entry_id.to_string()).bind(recording_id.map(|id| id.to_string())).bind(position as i64).execute(&mut **tx).await?;
        sqlx::query("INSERT INTO playlist_follow_entries(playlist_id,entry_id,occurrence_id,provider_recording_id,kind,title,artist) VALUES (?,?,?,?,?,?,?)
            ON CONFLICT(playlist_id,entry_id) DO UPDATE SET occurrence_id = excluded.occurrence_id,provider_recording_id = excluded.provider_recording_id,
                kind = excluded.kind,title = excluded.title,artist = excluded.artist")
            .bind(playlist).bind(entry_id.to_string()).bind(&entry.occurrence_id).bind(&provider_recording_id).bind(kind.as_str()).bind(&title).bind(&artist).execute(&mut **tx).await?;
        entries.push(ImportedPlaylistEntry {
            entry_id,
            recording_id,
            position: position as u32,
            origin: ImportedPlaylistEntryOrigin {
                kind,
                provider_recording_id,
                occurrence_id: entry.occurrence_id,
            },
            title,
            artist,
        });
    }
    sqlx::query("DELETE FROM playlist_follow_entries WHERE playlist_id = ? AND entry_id IN (SELECT id FROM playlist_entries WHERE playlist_id = ? AND deleted = 1)")
        .bind(playlist).bind(playlist).execute(&mut **tx).await?;
    sqlx::query("DELETE FROM playlist_entries WHERE playlist_id = ? AND deleted = 1")
        .bind(playlist)
        .execute(&mut **tx)
        .await?;
    Ok(ImportedPlaylistSnapshot {
        playlist_id: receipts::identity(playlist)?,
        name: snapshot.detail.name,
        origin: origin(tx, playlist).await?.ok_or(Error::NotFound)?,
        recordings,
        entries,
    })
}

async fn publish(
    connection: &mut SqliteConnection,
    account: &str,
    publication: CollectionPublication,
) -> Result<i64> {
    let id = match &publication {
        CollectionPublication::PlaylistFollowed { publication_id, .. }
        | CollectionPublication::PlaylistFollowRefreshed { publication_id, .. } => publication_id,
        CollectionPublication::PlaylistImported { .. } => {
            return Err(Error::Internal("Invalid follow publication".into()));
        }
    };
    let payload = import_publication::encode(&publication)?;
    Ok(sqlx::query(
        "INSERT INTO sync_events(user_id,origin,event_id,payload) VALUES (?,'playlist_follow',?,?)",
    )
    .bind(account)
    .bind(id.to_string())
    .bind(payload)
    .execute(connection)
    .await?
    .last_insert_rowid())
}

pub(in crate::collection) async fn origin(
    connection: &mut SqliteConnection,
    playlist: &str,
) -> Result<Option<ImportedPlaylistOrigin>> {
    let row = sqlx::query("SELECT installation_id,snapshot_installation_revision AS installation_revision,service_id,snapshot_service_revision AS service_revision,provider,provider_playlist_id,snapshot,attribution,created_at FROM playlist_follows WHERE playlist_id = ?")
        .bind(playlist).fetch_optional(connection).await?;
    row.map(|row| {
        let attribution: String = row.try_get("attribution")?;
        Ok(ImportedPlaylistOrigin {
            installation_id: row.try_get("installation_id")?,
            installation_revision: row.try_get("installation_revision")?,
            service_id: row.try_get("service_id")?,
            service_revision: row.try_get("service_revision")?,
            provider: row.try_get("provider")?,
            provider_playlist_id: row.try_get("provider_playlist_id")?,
            snapshot: row.try_get("snapshot")?,
            attribution: serde_json::from_str(&attribution)
                .map_err(|_| Error::Internal("Stored playlist attribution is invalid".into()))?,
            imported_at: row.try_get("created_at")?,
        })
    })
    .transpose()
}
