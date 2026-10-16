use super::{evidence, persist};
use crate::Result;
use hito_plugin_sdk::ProviderRecording;
use sqlx::{Sqlite, Transaction};

pub(crate) struct ProviderMetadata {
    pub(crate) recording_id: String,
    pub(crate) release_id: Option<String>,
    pub(crate) observation_id: String,
}

pub(crate) async fn publish(
    tx: &mut Transaction<'_, Sqlite>,
    library: &str,
    installation: &str,
    provider: &str,
    details: &ProviderRecording,
) -> Result<ProviderMetadata> {
    let release_id = persist::release(tx, library, installation, details.release.as_ref()).await?;
    let recording_id = persist::recording(tx, library, installation, details).await?;
    let observation_id = evidence::observe(
        tx,
        installation,
        provider,
        &recording_id,
        release_id.as_deref(),
        details,
    )
    .await?;
    sqlx::query("UPDATE recordings SET release_id = ?, title = ?, artist = ?, duration_ms = ?, metadata_observation_id = ? WHERE id = ?")
        .bind(&release_id).bind(&details.title).bind(&details.artist).bind(details.duration_ms.unwrap_or(0) as i64)
        .bind(&observation_id).bind(&recording_id).execute(&mut **tx).await?;
    Ok(ProviderMetadata {
        recording_id,
        release_id,
        observation_id,
    })
}
