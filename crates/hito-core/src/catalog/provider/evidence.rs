use super::bindings::Binding;
use crate::Result;
use hito_plugin_sdk::ProviderRecording;
use sha2::{Digest, Sha256};
use sqlx::{Sqlite, Transaction};
use uuid::Uuid;

#[tracing::instrument(level = "debug", skip_all, err(Debug))]
pub(super) async fn observe(
    tx: &mut Transaction<'_, Sqlite>,
    binding: &Binding,
    provider: &str,
    release: Option<&str>,
    details: &ProviderRecording,
) -> Result<String> {
    let metadata = serde_json::to_string(details)
        .map_err(|_| crate::Error::Internal("Provider metadata could not be encoded".into()))?;
    let digest = format!("{:x}", Sha256::digest(metadata.as_bytes()));
    let inserted = sqlx::query("INSERT INTO metadata_observations(id,recording_id,release_id,provider,origin,observed_at,title,artist,album,album_artist,duration_ms,disc_number,track_number,plugin_installation_id,provider_recording_id,provider_revision,provider_metadata,provider_digest,provider_binding_id)
        VALUES (?,?,?,?,'provider_response',unixepoch(),?,?,?,?,?,?,?,?,?,?,?,?,?) ON CONFLICT(provider_binding_id,provider_digest) DO NOTHING")
        .bind(Uuid::new_v4().to_string()).bind(&binding.recording_id).bind(release).bind(provider)
        .bind(&details.title).bind(&details.artist).bind(details.release.as_ref().map(|r| &r.title)).bind(details.release.as_ref().and_then(|r| r.artist.as_ref()))
        .bind(details.duration_ms.unwrap_or(0) as i64).bind(details.disc_number.map(i64::from)).bind(details.track_number.map(i64::from))
        .bind(&binding.installation_id).bind(&details.id).bind(&details.revision).bind(metadata).bind(&digest).bind(&binding.id).execute(&mut **tx).await?.rows_affected();
    tracing::debug!(created = inserted == 1, "Provider observation retained");
    Ok(sqlx::query_scalar("SELECT id FROM metadata_observations WHERE provider_binding_id = ? AND provider_digest = ?")
        .bind(&binding.id).bind(digest).fetch_one(&mut **tx).await?)
}
