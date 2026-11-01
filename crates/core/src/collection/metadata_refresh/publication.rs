use super::{
    ImportedPlaylistMetadataRefresh,
    authority::{Scope, identity},
};
use crate::collection::{
    CollectionPublication, ImportedPlaylistRecording, PlaylistMetadataRefreshOrigin,
    import_publication,
};
use crate::{
    Actor, Error, Result, catalog,
    plugins::{PlaylistOperation, PlaylistSnapshot},
};
use hito_plugin_sdk::{ProviderPlaylistItem, ProviderRelease};
use sqlx::{Sqlite, Transaction};
use std::collections::HashMap;
use uuid::Uuid;

pub(super) async fn publish(
    tx: &mut Transaction<'_, Sqlite>,
    actor: &Actor,
    playlist_id: Uuid,
    request_id: Uuid,
    scope: &Scope,
    operation: &PlaylistOperation,
    snapshot: PlaylistSnapshot,
) -> Result<(ImportedPlaylistMetadataRefresh, Option<i64>)> {
    let mut metadata = HashMap::new();
    for entry in snapshot.entries {
        if let ProviderPlaylistItem::Recording { recording } = entry.item {
            metadata.entry(recording.id.clone()).or_insert(*recording);
        }
    }
    let mut receipt = ImportedPlaylistMetadataRefresh {
        request_id,
        playlist_id,
        updated_recordings: 0,
        unchanged_recordings: 0,
        unavailable_recordings: 0,
    };
    let mut recordings = Vec::new();
    let mut releases: HashMap<String, ProviderRelease> = HashMap::new();
    for target in &scope.targets {
        let Some(details) = metadata.get(&target.provider_recording_id) else {
            receipt.unavailable_recordings += 1;
            continue;
        };
        let Some(release) = details.release.as_ref() else {
            receipt.unchanged_recordings += 1;
            continue;
        };
        if let Some(previous) = releases.insert(release.id.clone(), release.clone())
            && previous != *release
        {
            return Err(Error::Upstream(
                "The provider returned conflicting release metadata",
            ));
        }
        let published = catalog::refresh_existing_provider_metadata(
            tx,
            actor,
            &operation.installation,
            &operation.provider,
            target,
            details,
        )
        .await?;
        recordings.push(ImportedPlaylistRecording {
            recording_id: identity(&published.recording_id)?,
            library_id: identity(&target.library_id)?,
            release_id: published.release_id.as_deref().map(identity).transpose()?,
            observation_id: identity(&published.observation_id)?,
            metadata: details.clone(),
        });
        receipt.updated_recordings += 1;
    }
    if recordings.is_empty() {
        return Ok((receipt, None));
    }
    let observed_at = sqlx::query_scalar("SELECT unixepoch()")
        .fetch_one(&mut **tx)
        .await?;
    let publication_id = Uuid::new_v4();
    let publication = CollectionPublication::PlaylistMetadataRefreshed {
        publication_id,
        playlist_id,
        origin: PlaylistMetadataRefreshOrigin {
            installation_id: operation.installation.clone(),
            installation_revision: operation.installation_revision,
            service_id: operation.service.clone(),
            service_revision: operation.service_revision,
            provider: operation.provider.clone(),
            provider_playlist_id: snapshot.detail.id,
            snapshot: snapshot.detail.snapshot,
            attribution: snapshot.detail.attribution,
            observed_at,
        },
        recordings,
    };
    let payload = import_publication::encode(&publication)?;
    let sequence = sqlx::query(
        "INSERT INTO sync_events(user_id,origin,event_id,payload) VALUES (?,'playlist_metadata',?,?)",
    ).bind(actor.id()).bind(publication_id.to_string()).bind(payload).execute(&mut **tx).await?.last_insert_rowid();
    Ok((receipt, Some(sequence)))
}
