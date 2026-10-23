use super::{
    CollectionPublication, ImportProviderPlaylist, ImportedPlaylistEntry,
    ImportedPlaylistEntryKind, ImportedPlaylistEntryOrigin, ImportedPlaylistOrigin,
    ImportedPlaylistRecording, ImportedPlaylistSnapshot, PlaylistImportReceipt, import_publication,
    import_receipts,
};
use crate::{
    Actor, Error, Result, catalog,
    plugins::{Operation, PlaylistSnapshot},
};
use hito_plugin_sdk::ProviderPlaylistItem;
use sqlx::{Row, Sqlite, SqliteConnection, Transaction};
use std::collections::HashMap;
use uuid::Uuid;

pub(super) async fn publish(
    tx: &mut Transaction<'_, Sqlite>,
    actor: &Actor,
    installation: &str,
    request: &ImportProviderPlaylist,
    operation: &Operation,
    snapshot: PlaylistSnapshot,
    digest: &str,
) -> Result<PlaylistImportReceipt> {
    let playlist_id = Uuid::new_v4();
    let publication_id = Uuid::new_v4();
    let library = crate::identity::personal_library(tx, actor.id()).await?;
    let library_id = identity(&library)?;
    let imported_at = sqlx::query_scalar("SELECT unixepoch()")
        .fetch_one(&mut **tx)
        .await?;
    let origin = ImportedPlaylistOrigin {
        installation_id: installation.into(),
        installation_revision: request.installation_revision,
        service_id: operation.service.clone(),
        service_revision: operation.service_revision,
        provider: operation.provider.clone(),
        provider_playlist_id: snapshot.detail.id,
        snapshot: snapshot.detail.snapshot,
        attribution: snapshot.detail.attribution,
        imported_at,
    };
    sqlx::query("INSERT INTO playlists(id, user_id, name, kind) VALUES (?, ?, ?, 'local')")
        .bind(playlist_id.to_string())
        .bind(actor.id())
        .bind(&snapshot.detail.name)
        .execute(&mut **tx)
        .await?;
    let attribution = serde_json::to_string(&origin.attribution)
        .map_err(|_| Error::Internal("Playlist attribution could not be encoded".into()))?;
    sqlx::query("INSERT INTO playlist_imports(playlist_id, installation_id, installation_revision, service_id, service_revision, provider, provider_playlist_id, snapshot, attribution, imported_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)")
        .bind(playlist_id.to_string()).bind(installation).bind(request.installation_revision)
        .bind(&origin.service_id).bind(origin.service_revision).bind(&origin.provider)
        .bind(&origin.provider_playlist_id).bind(&origin.snapshot).bind(attribution).bind(imported_at)
        .execute(&mut **tx).await?;
    let mut identities = HashMap::new();
    let mut recordings = Vec::new();
    let mut entries = Vec::with_capacity(snapshot.entries.len());
    for (position, entry) in snapshot.entries.into_iter().enumerate() {
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
                        installation,
                        &operation.provider,
                        &metadata,
                    )
                    .await?;
                    crate::metadata::refresh_recording_credits(tx, &published.recording_id).await?;
                    if let Some(release) = &published.release_id {
                        crate::metadata::refresh_release(tx, actor.id(), release).await?;
                    }
                    let recording_id = identity(&published.recording_id)?;
                    identities.insert(remote.clone(), recording_id);
                    recordings.push(ImportedPlaylistRecording {
                        recording_id,
                        library_id,
                        release_id: published.release_id.as_deref().map(identity).transpose()?,
                        observation_id: identity(&published.observation_id)?,
                        metadata,
                    });
                    recording_id
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
        let entry_id = Uuid::new_v4();
        sqlx::query("INSERT INTO playlist_entries(playlist_id, id, recording_id, position) VALUES (?, ?, ?, ?)")
            .bind(playlist_id.to_string()).bind(entry_id.to_string()).bind(recording_id.map(|id| id.to_string()))
            .bind(position as i64).execute(&mut **tx).await?;
        sqlx::query("INSERT INTO playlist_import_entries(playlist_id, entry_id, occurrence_id, provider_recording_id, kind, title, artist) VALUES (?, ?, ?, ?, ?, ?, ?)")
            .bind(playlist_id.to_string()).bind(entry_id.to_string()).bind(&entry.occurrence_id)
            .bind(&provider_recording_id).bind(kind.as_str()).bind(&title).bind(&artist)
            .execute(&mut **tx).await?;
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
    let publication = CollectionPublication::PlaylistImported {
        publication_id,
        snapshot: ImportedPlaylistSnapshot {
            playlist_id,
            name: snapshot.detail.name,
            origin,
            recordings,
            entries,
        },
    };
    let payload = import_publication::encode(&publication)?;
    let publication_sequence = sqlx::query("INSERT INTO sync_events(user_id, origin, event_id, payload) VALUES (?, 'playlist_import', ?, ?)")
        .bind(actor.id()).bind(publication_id.to_string()).bind(payload).execute(&mut **tx)
        .await?.last_insert_rowid();
    let receipt = PlaylistImportReceipt {
        request_id: request.request_id,
        playlist_id,
    };
    import_receipts::save(tx, actor.id(), digest, &receipt, publication_sequence).await?;
    Ok(receipt)
}

pub(super) async fn origin(
    connection: &mut SqliteConnection,
    playlist: &str,
) -> Result<Option<ImportedPlaylistOrigin>> {
    let row = sqlx::query("SELECT installation_id, installation_revision, service_id, service_revision, provider, provider_playlist_id, snapshot, attribution, imported_at FROM playlist_imports WHERE playlist_id = ?")
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
            imported_at: row.try_get("imported_at")?,
        })
    })
    .transpose()
}

fn identity(value: &str) -> Result<Uuid> {
    Uuid::parse_str(value)
        .map_err(|_| Error::Internal("Stored provider identity is invalid".into()))
}
