use super::{
    identity_authority::{RecordingTarget, workflow},
    identity_types::*,
};
use crate::{Actor, Catalog, Error, Result};
use hito_plugin_sdk::{Capability, Manifest, ProviderRecordingRef};
use std::time::Duration;

const MAX_MANIFEST_BYTES: i64 = 65_536;

#[derive(sqlx::FromRow)]
struct StoredIdentity {
    active: bool,
    id: String,
    revision: i64,
    installation_id: String,
    installation_revision: i64,
    provider_recording_id: String,
    provider_revision: Option<String>,
    manifest: Option<String>,
}

fn reviewed_identity(row: StoredIdentity) -> Result<RecordingLyricIdentity> {
    let manifest: Manifest = row
        .manifest
        .as_deref()
        .and_then(|value| serde_json::from_str(value).ok())
        .filter(|manifest: &Manifest| manifest.validate().is_ok())
        .ok_or_else(|| Error::Internal("Stored lyric service manifest is invalid".into()))?;
    let reference = ProviderRecordingRef {
        id: row.provider_recording_id,
        revision: row.provider_revision,
    };
    if !super::valid_id(&row.id) || row.revision < 1 || reference.validate().is_err() {
        return Err(Error::Internal("Stored lyric identity is invalid".into()));
    }
    Ok(RecordingLyricIdentity {
        selectable: row.active && manifest.capabilities.contains(&Capability::Lyrics),
        selector: RecordingLyricSelector::Reviewed {
            identity_id: row.id,
            identity_revision: row.revision,
        },
        installation_id: row.installation_id,
        installation_revision: row.installation_revision,
        provider_recording: reference,
        label: manifest.name,
    })
}

impl Catalog {
    pub async fn recording_lyric_identities(
        &self,
        actor: &Actor,
        recording: &str,
    ) -> Result<RecordingLyricIdentities> {
        workflow("lyric_identities", Duration::from_secs(1), async {
            let target = RecordingTarget::capture(
                &mut *self.store.pool.acquire().await?, actor, recording,
            ).await?;
            let mut items = Vec::new();
            if target.personal {
                for item in self.recording_lyrics_providers(actor, recording).await?.items {
                    items.push(RecordingLyricIdentity {
                        selectable: true,
                        selector: RecordingLyricSelector::Observation {
                            observation_id: item.observation_id,
                        },
                        installation_id: item.installation_id,
                        installation_revision: item.installation_revision,
                        provider_recording: item.provider_recording,
                        label: item.label,
                    });
                }
            }
            let mut tx = self.store.pool.begin().await?;
            target.recheck(&mut tx, actor, recording).await?;
            let rows: Vec<StoredIdentity> = sqlx::query_as("SELECT identity.id,identity.revision,identity.installation_id,installation.revision AS installation_revision,installation.active AND service.active AS active,
                identity.provider_recording_id,identity.provider_revision,
                CASE WHEN length(CAST(installation.manifest AS BLOB))<=? THEN installation.manifest END AS manifest
                FROM recording_lyric_identities identity JOIN plugin_installations installation ON installation.id=identity.installation_id AND installation.user_id=identity.user_id
                JOIN plugin_services service ON service.id=installation.service_id
                WHERE identity.user_id=? AND identity.recording_id=? ORDER BY identity.created_at,identity.id LIMIT 9")
                .bind(MAX_MANIFEST_BYTES)
                .bind(actor.id())
                .bind(recording)
                .fetch_all(&mut *tx)
                .await?;
            if rows.len() > 8 {
                return Err(Error::Internal("Stored lyric identity limit is invalid".into()));
            }
            for row in rows {
                items.push(reviewed_identity(row)?);
            }
            target.recheck(&mut tx, actor, recording).await?;
            tx.commit().await?;
            actor.check_session(&self.store).await?;
            Ok(RecordingLyricIdentities {
                recording_id: recording.into(),
                items,
            })
        })
        .await
    }

    pub async fn remove_recording_lyric_identity(
        &self,
        actor: &Actor,
        recording: &str,
        identity: &str,
        revision: i64,
    ) -> Result<()> {
        workflow("lyric_identity_remove", Duration::from_secs(1), async {
            if !super::valid_id(identity) || revision < 1 {
                return Err(Error::Invalid("Use a current lyric identity revision".into()));
            }
            let mut tx = self.store.pool.begin_with("BEGIN IMMEDIATE").await?;
            let target = RecordingTarget::capture(&mut tx, actor, recording).await?;
            let removed = sqlx::query("DELETE FROM recording_lyric_identities WHERE id=? AND revision=? AND user_id=? AND recording_id=?")
                .bind(identity)
                .bind(revision)
                .bind(actor.id())
                .bind(recording)
                .execute(&mut *tx)
                .await?
                .rows_affected();
            if removed != 1 {
                return Err(Error::NotFound);
            }
            target.recheck(&mut tx, actor, recording).await?;
            tx.commit().await?;
            actor.check_session(&self.store).await?;
            Ok(())
        })
        .await
    }
}
