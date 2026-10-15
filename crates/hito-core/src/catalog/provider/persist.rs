use crate::{Error, Result};
use hito_plugin_sdk::{ProviderRecording, ProviderRelease, SourcePage};
use sqlx::{Sqlite, Transaction};
use uuid::Uuid;

pub(super) async fn library(tx: &mut Transaction<'_, Sqlite>, owner: &str) -> Result<String> {
    sqlx::query("INSERT INTO libraries(id,name,owner_id) VALUES (?,'Personal providers',?) ON CONFLICT(owner_id) WHERE owner_id IS NOT NULL DO NOTHING")
        .bind(Uuid::new_v4().to_string()).bind(owner).execute(&mut **tx).await?;
    let id: String = sqlx::query_scalar("SELECT id FROM libraries WHERE owner_id = ?")
        .bind(owner)
        .fetch_one(&mut **tx)
        .await?;
    sqlx::query("INSERT OR IGNORE INTO library_members(library_id,user_id) VALUES (?,?)")
        .bind(&id)
        .bind(owner)
        .execute(&mut **tx)
        .await?;
    Ok(id)
}

pub(super) async fn release(
    tx: &mut Transaction<'_, Sqlite>,
    library: &str,
    installation: &str,
    details: Option<&ProviderRelease>,
) -> Result<Option<String>> {
    let Some(details) = details else {
        return Ok(None);
    };
    let key = serde_json::to_string(&("plugin", installation, &details.id))
        .map_err(|_| Error::Internal("Provider release identity could not be encoded".into()))?;
    sqlx::query("INSERT INTO releases(id,library_id,title,artist,identity_key) VALUES (?,?,?,?,?) ON CONFLICT(library_id,identity_key) DO UPDATE SET title = excluded.title,artist = excluded.artist")
        .bind(Uuid::new_v4().to_string()).bind(library).bind(&details.title).bind(&details.artist).bind(&key).execute(&mut **tx).await?;
    Ok(
        sqlx::query_scalar("SELECT id FROM releases WHERE library_id = ? AND identity_key = ?")
            .bind(library)
            .bind(key)
            .fetch_optional(&mut **tx)
            .await?,
    )
}

pub(super) async fn recording(
    tx: &mut Transaction<'_, Sqlite>,
    library: &str,
    installation: &str,
    details: &ProviderRecording,
) -> Result<String> {
    if let Some(id) = sqlx::query_scalar("SELECT recording_id FROM provider_items WHERE installation_id = ? AND provider_recording_id = ?")
        .bind(installation).bind(&details.id).fetch_optional(&mut **tx).await? { return Ok(id); }
    let id = Uuid::new_v4().to_string();
    sqlx::query("INSERT INTO recordings(id,library_id,title,artist,provenance) VALUES (?,?,?,?,'provider_response')")
        .bind(&id).bind(library).bind(&details.title).bind(&details.artist).execute(&mut **tx).await?;
    sqlx::query("INSERT INTO provider_items(installation_id,provider_recording_id,recording_id) VALUES (?,?,?)")
        .bind(installation).bind(&details.id).bind(&id).execute(&mut **tx).await?;
    Ok(id)
}

pub(super) async fn sources(
    tx: &mut Transaction<'_, Sqlite>,
    installation: &str,
    recording: &str,
    release: Option<&str>,
    observation: &str,
    details: &ProviderRecording,
    page: &SourcePage,
) -> Result<()> {
    let generation =
        sqlx::query_scalar("SELECT generation FROM provider_source_state WHERE recording_id = ?")
            .bind(recording)
            .fetch_one(&mut **tx)
            .await?;
    let ids = super::sources::replace(
        tx,
        installation,
        recording,
        page,
        super::sources::Snapshot {
            generation,
            observation,
        },
    )
    .await?;
    for id in ids {
        sqlx::query("INSERT INTO source_metadata(source_id,release_id,title,artist,duration_ms,disc_number,track_number,inspection_version,observation_id,provenance) VALUES (?,?,?,?,?,?,?,0,?,'provider_response')
            ON CONFLICT(source_id) DO UPDATE SET release_id = excluded.release_id,title = excluded.title,artist = excluded.artist,duration_ms = excluded.duration_ms,disc_number = excluded.disc_number,track_number = excluded.track_number,observation_id = excluded.observation_id,provenance = excluded.provenance")
            .bind(&id).bind(release).bind(&details.title).bind(&details.artist).bind(details.duration_ms.unwrap_or(0) as i64)
            .bind(details.disc_number.map(i64::from)).bind(details.track_number.map(i64::from)).bind(observation).execute(&mut **tx).await?;
        super::sources::project(tx, &id).await?;
    }
    Ok(())
}
