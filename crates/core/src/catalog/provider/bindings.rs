use crate::{Error, Result};
use hito_plugin_sdk::ProviderRecording;
use sqlx::{Sqlite, Transaction};
use uuid::Uuid;

pub(crate) const MAX_PLAYBACK_BINDINGS: usize = 8;

#[derive(sqlx::FromRow)]
pub(super) struct Binding {
    pub id: String,
    pub installation_id: String,
    pub recording_id: String,
    pub revision: i64,
}

pub(super) async fn current(
    tx: &mut Transaction<'_, Sqlite>,
    installation: &str,
    provider_recording: &str,
) -> Result<Option<Binding>> {
    Ok(sqlx::query_as("SELECT id,installation_id,recording_id,revision FROM provider_items WHERE installation_id = ? AND provider_recording_id = ? AND retired = 0")
        .bind(installation).bind(provider_recording).fetch_optional(&mut **tx).await?)
}

pub(super) async fn adopt(
    tx: &mut Transaction<'_, Sqlite>,
    library: &str,
    installation: &str,
    details: &ProviderRecording,
) -> Result<Binding> {
    if let Some(binding) = current(tx, installation, &details.id).await? {
        return Ok(binding);
    }
    let recording = Uuid::new_v4().to_string();
    sqlx::query("INSERT INTO recordings(id,library_id,title,artist,provenance) VALUES (?,?,?,?,'provider_response')")
        .bind(&recording).bind(library).bind(&details.title).bind(&details.artist).execute(&mut **tx).await?;
    let binding = insert(
        tx,
        installation,
        &details.id,
        &recording,
        "primary",
        "provider_identity",
    )
    .await?;
    sqlx::query("UPDATE recordings SET metadata_binding_id = ? WHERE id = ?")
        .bind(&binding.id)
        .bind(&recording)
        .execute(&mut **tx)
        .await?;
    Ok(binding)
}

pub(super) async fn attach(
    tx: &mut Transaction<'_, Sqlite>,
    installation: &str,
    provider_recording: &str,
    recording: &str,
) -> Result<Binding> {
    if current(tx, installation, provider_recording)
        .await?
        .is_some()
    {
        return Err(Error::Conflict(
            "This provider recording is already bound. Review its existing recording.".into(),
        ));
    }
    insert(
        tx,
        installation,
        provider_recording,
        recording,
        "source",
        "user_confirmed",
    )
    .await
}

async fn insert(
    tx: &mut Transaction<'_, Sqlite>,
    installation: &str,
    provider_recording: &str,
    recording: &str,
    role: &str,
    evidence: &str,
) -> Result<Binding> {
    let binding = Binding {
        id: Uuid::new_v4().to_string(),
        installation_id: installation.into(),
        recording_id: recording.into(),
        revision: 1,
    };
    sqlx::query("INSERT INTO provider_items(id,installation_id,provider_recording_id,recording_id,role,evidence) VALUES (?,?,?,?,?,?)")
        .bind(&binding.id).bind(installation).bind(provider_recording).bind(recording).bind(role).bind(evidence)
        .execute(&mut **tx).await?;
    Ok(binding)
}

impl Binding {
    pub(super) async fn recheck(&self, tx: &mut Transaction<'_, Sqlite>) -> Result<()> {
        let current: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM provider_items WHERE id = ? AND recording_id = ? AND revision = ? AND retired = 0)")
            .bind(&self.id).bind(&self.recording_id).bind(self.revision).fetch_one(&mut **tx).await?;
        if !current {
            return Err(Error::Conflict(
                "This provider binding changed. Review it again.".into(),
            ));
        }
        Ok(())
    }

    pub(super) async fn enroll_sources(
        &self,
        tx: &mut Transaction<'_, Sqlite>,
        observation: &str,
    ) -> Result<i64> {
        self.recheck(tx).await?;
        if let Some(generation) =
            sqlx::query_scalar("SELECT generation FROM provider_source_state WHERE binding_id = ?")
                .bind(&self.id)
                .fetch_optional(&mut **tx)
                .await?
        {
            return Ok(generation);
        }
        let count: i64 = sqlx::query_scalar("SELECT count(*) FROM provider_items item JOIN provider_source_state state ON state.binding_id = item.id WHERE item.recording_id = ? AND item.retired = 0")
            .bind(&self.recording_id).fetch_one(&mut **tx).await?;
        if count >= MAX_PLAYBACK_BINDINGS as i64 {
            return Err(Error::Conflict(
                "This recording already has the maximum number of playback bindings.".into(),
            ));
        }
        sqlx::query("INSERT INTO provider_source_state(binding_id,observation_id) VALUES (?,?)")
            .bind(&self.id)
            .bind(observation)
            .execute(&mut **tx)
            .await?;
        Ok(0)
    }
}
