use super::{Completion, RecordingLyricsProvider, RecordingLyricsProviders, valid_id};
use crate::{
    Actor, Catalog, Error, Result, catalog::MAX_PLAYBACK_BINDINGS, identity::recording_access,
};
use hito_plugin_sdk::{Capability, Manifest, ProviderRecordingRef};
use std::{collections::BTreeSet, time::Duration};
use tracing::Instrument;

const MAX_IDENTITIES: usize = MAX_PLAYBACK_BINDINGS + 1;
const MAX_MANIFEST_BYTES: usize = 65_536;

#[derive(sqlx::FromRow)]
struct Stored {
    observation_id: String,
    installation_id: String,
    installation_revision: i64,
    provider_recording_id: String,
    provider_revision: Option<String>,
    manifest: Option<String>,
}

impl Catalog {
    pub async fn recording_lyrics_providers(
        &self,
        actor: &Actor,
        recording: &str,
    ) -> Result<RecordingLyricsProviders> {
        let mut completion = Completion::providers();
        let result = tokio::time::timeout(
            Duration::from_secs(1),
            self.read_lyric_providers(actor, recording),
        )
        .instrument(completion.span.clone())
        .await
        .unwrap_or(Err(Error::Upstream(
            "Lyric providers could not be read in time",
        )));
        completion.outcome = result
            .as_ref()
            .map(|_| "read")
            .unwrap_or_else(|error| error.code());
        result
    }

    async fn read_lyric_providers(
        &self,
        actor: &Actor,
        recording: &str,
    ) -> Result<RecordingLyricsProviders> {
        if !valid_id(recording) {
            return Err(Error::Invalid("Use a valid recording identity".into()));
        }
        let mut tx = self.store.pool.begin().await?;
        actor.check_session_on(&mut tx).await?;
        recording_access(&mut tx, actor, recording).await?;
        let personal: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM recordings recording
                JOIN libraries library ON library.id=recording.library_id AND library.owner_id=?
                JOIN library_access_grants access ON access.library_id=library.id AND access.user_id=?
                WHERE recording.id=?)",
        )
        .bind(actor.id())
        .bind(actor.id())
        .bind(recording)
        .fetch_one(&mut *tx)
        .await?;
        if !personal {
            return Err(Error::NotFound);
        }
        let rows: Vec<Stored> = sqlx::query_as(
            "SELECT observation.id AS observation_id,binding.installation_id,
                installation.revision AS installation_revision,observation.provider_recording_id,
                observation.provider_revision,
                CASE WHEN length(CAST(installation.manifest AS BLOB))<=? THEN installation.manifest END AS manifest
            FROM provider_items binding
            JOIN recordings recording ON recording.id=binding.recording_id
            JOIN plugin_installations installation ON installation.id=binding.installation_id
                AND installation.user_id=? AND installation.active=1
            JOIN plugin_services service ON service.id=installation.service_id AND service.active=1
            LEFT JOIN provider_source_state source_state ON source_state.binding_id=binding.id
            JOIN metadata_observations observation ON observation.id=CASE WHEN binding.role='primary'
                THEN recording.metadata_observation_id ELSE source_state.observation_id END
                AND observation.recording_id=recording.id AND observation.provider_binding_id=binding.id
                AND observation.plugin_installation_id=binding.installation_id
                AND observation.provider_recording_id=binding.provider_recording_id
                AND observation.origin='provider_response' AND observation.source_id IS NULL
            WHERE binding.recording_id=? AND binding.retired=0
                AND ((binding.role='primary' AND recording.metadata_binding_id=binding.id)
                    OR binding.role='source')
            ORDER BY binding.role='primary' DESC,binding.id,observation.id LIMIT ?",
        )
        .bind(MAX_MANIFEST_BYTES as i64)
        .bind(actor.id())
        .bind(recording)
        .bind((MAX_IDENTITIES + 1) as i64)
        .fetch_all(&mut *tx)
        .await?;
        if rows.len() > MAX_IDENTITIES {
            return Err(Error::Conflict(
                "This recording has too many current provider identities".into(),
            ));
        }
        let mut seen = BTreeSet::new();
        let mut items = Vec::new();
        for row in rows {
            let manifest: Manifest = row
                .manifest
                .as_deref()
                .filter(|bytes| bytes.len() <= MAX_MANIFEST_BYTES)
                .and_then(|bytes| serde_json::from_str(bytes).ok())
                .filter(|manifest: &Manifest| manifest.validate().is_ok())
                .ok_or_else(|| {
                    Error::Internal("Stored lyric provider manifest is invalid".into())
                })?;
            if !manifest.capabilities.contains(&Capability::Lyrics) {
                continue;
            }
            let provider_recording = ProviderRecordingRef {
                id: row.provider_recording_id,
                revision: row.provider_revision,
            };
            if provider_recording.validate().is_err()
                || !valid_id(&row.observation_id)
                || !valid_id(&row.installation_id)
                || row.installation_revision < 1
            {
                return Err(Error::Internal(
                    "Stored lyric provider identity is invalid".into(),
                ));
            }
            if seen.insert(row.observation_id.clone()) {
                items.push(RecordingLyricsProvider {
                    observation_id: row.observation_id,
                    installation_id: row.installation_id,
                    installation_revision: row.installation_revision,
                    provider_recording,
                    label: manifest.name,
                });
            }
        }
        actor.check_session_on(&mut tx).await?;
        tx.commit().await?;
        actor.check_session(&self.store).await?;
        Ok(RecordingLyricsProviders {
            recording_id: recording.into(),
            items,
        })
    }
}
