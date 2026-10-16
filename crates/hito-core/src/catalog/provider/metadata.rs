use super::{bindings, evidence, persist};
use crate::Result;
use hito_plugin_sdk::ProviderRecording;
use sqlx::{Sqlite, Transaction};

pub(crate) struct ProviderMetadata {
    pub(super) binding: bindings::Binding,
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
    let binding = bindings::adopt(tx, library, installation, details).await?;
    let owns_metadata: bool =
        sqlx::query_scalar("SELECT metadata_binding_id IS ? FROM recordings WHERE id = ?")
            .bind(&binding.id)
            .bind(&binding.recording_id)
            .fetch_one(&mut **tx)
            .await?;
    let publication = if owns_metadata {
        persist::ReleasePublication::PublishInherited
    } else {
        persist::ReleasePublication::PreserveExisting
    };
    let release_id = persist::release(
        tx,
        library,
        installation,
        details.release.as_ref(),
        publication,
    )
    .await?;
    let observation_id =
        evidence::observe(tx, &binding, provider, release_id.as_deref(), details).await?;
    sqlx::query("UPDATE recordings SET release_id = ?, title = ?, artist = ?, duration_ms = ?, metadata_observation_id = ? WHERE id = ? AND metadata_binding_id = ?")
        .bind(&release_id).bind(&details.title).bind(&details.artist).bind(details.duration_ms.unwrap_or(0) as i64)
        .bind(&observation_id).bind(&binding.recording_id).bind(&binding.id).execute(&mut **tx).await?;
    Ok(ProviderMetadata {
        recording_id: binding.recording_id.clone(),
        binding,
        release_id,
        observation_id,
    })
}
