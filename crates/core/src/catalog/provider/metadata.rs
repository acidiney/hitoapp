use super::{bindings, evidence, persist};
use crate::{Actor, Error, Result};
use hito_plugin_sdk::ProviderRecording;
use sqlx::{Sqlite, Transaction};

pub(crate) struct ProviderMetadata {
    pub(super) binding: bindings::Binding,
    pub(crate) recording_id: String,
    pub(crate) release_id: Option<String>,
    pub(crate) observation_id: String,
}

#[derive(sqlx::FromRow, PartialEq, Eq)]
pub(crate) struct ExistingProviderMetadata {
    pub(crate) recording_id: String,
    pub(crate) library_id: String,
    pub(crate) access_grant: String,
    pub(crate) binding_id: String,
    pub(crate) binding_revision: i64,
    pub(crate) provider_recording_id: String,
    pub(crate) observation_id: String,
}

pub(crate) async fn refresh_existing(
    tx: &mut Transaction<'_, Sqlite>,
    actor: &Actor,
    installation: &str,
    provider: &str,
    target: &ExistingProviderMetadata,
    details: &ProviderRecording,
) -> Result<ProviderMetadata> {
    details
        .validate()
        .map_err(|_| Error::Upstream("The provider returned invalid recording metadata"))?;
    if details.id != target.provider_recording_id || details.release.is_none() {
        return Err(Error::Upstream(
            "The provider returned incompatible recording metadata",
        ));
    }
    actor.check_session_on(tx).await?;
    let current: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM recordings r
         JOIN libraries library ON library.id = r.library_id AND library.owner_id = ?
         JOIN library_access_grants access ON access.library_id = library.id AND access.user_id = ? AND access.id = ?
         JOIN provider_items binding ON binding.id = r.metadata_binding_id AND binding.recording_id = r.id
            AND binding.id = ? AND binding.revision = ? AND binding.installation_id = ?
            AND binding.provider_recording_id = ? AND binding.role = 'primary' AND binding.retired = 0
         JOIN metadata_observations observation ON observation.id = r.metadata_observation_id
            AND observation.id = ? AND observation.recording_id = r.id AND observation.provider_binding_id = binding.id
            AND observation.plugin_installation_id = binding.installation_id AND observation.provider_recording_id = binding.provider_recording_id
            AND observation.origin = 'provider_response' AND observation.provider = ?
         WHERE r.id = ? AND r.library_id = ? AND r.release_id IS NULL)",
    ).bind(actor.id()).bind(actor.id()).bind(&target.access_grant).bind(&target.binding_id)
        .bind(target.binding_revision).bind(installation).bind(&target.provider_recording_id)
        .bind(&target.observation_id).bind(provider).bind(&target.recording_id).bind(&target.library_id)
        .fetch_one(&mut **tx).await?;
    if !current {
        return Err(Error::Conflict(
            "This recording's metadata owner changed. Review it again.".into(),
        ));
    }
    let binding = bindings::Binding {
        id: target.binding_id.clone(),
        installation_id: installation.into(),
        recording_id: target.recording_id.clone(),
        revision: target.binding_revision,
    };
    let release_id = persist::release(
        tx,
        &target.library_id,
        installation,
        details.release.as_ref(),
        persist::ReleasePublication::PublishInherited,
    )
    .await?;
    let observation_id =
        evidence::observe(tx, &binding, provider, release_id.as_deref(), details).await?;
    let changed = sqlx::query("UPDATE recordings SET release_id = ?,title = ?,artist = ?,duration_ms = ?,metadata_observation_id = ?
        WHERE id = ? AND metadata_binding_id = ? AND metadata_observation_id = ? AND release_id IS NULL")
        .bind(&release_id).bind(&details.title).bind(&details.artist).bind(details.duration_ms.unwrap_or(0) as i64)
        .bind(&observation_id).bind(&target.recording_id).bind(&binding.id).bind(&target.observation_id)
        .execute(&mut **tx).await?.rows_affected();
    if changed != 1 {
        return Err(Error::Conflict(
            "This recording's metadata owner changed. Review it again.".into(),
        ));
    }
    crate::metadata::refresh_recording_credits(tx, &target.recording_id).await?;
    if let Some(release) = &release_id {
        crate::metadata::refresh_release(tx, actor.id(), release).await?;
    }
    Ok(ProviderMetadata {
        binding,
        recording_id: target.recording_id.clone(),
        release_id,
        observation_id,
    })
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
