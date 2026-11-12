use super::{
    identity_authority::{RecordingTarget, workflow},
    identity_services::{REQUIRED, validate_installation},
    identity_types::*,
};
use crate::{Actor, Catalog, Error, ProviderCandidate, Result, plugins::Operation};
use hito_plugin_sdk::{ProviderRecordingRef, RecordingMetadata, RecordingRequest};
use sha2::{Digest, Sha256};
use uuid::Uuid;

impl Catalog {
    pub async fn review_recording_lyric_identity(
        &self,
        actor: &Actor,
        recording: &str,
        candidate: ProviderCandidate,
    ) -> Result<RecordingLyricReview> {
        workflow("lyric_identity_review", super::LOOKUP_BUDGET, async {
            let target =
                RecordingTarget::capture(&mut *self.store.pool.acquire().await?, actor, recording)
                    .await?;
            let (operation, details) = self
                .lyric_candidate(actor, recording, &target, &candidate)
                .await?;
            let mut tx = self.store.pool.begin().await?;
            target.recheck(&mut tx, actor, recording).await?;
            operation.recheck(&mut tx).await?;
            let review_stamp = stamp(actor, recording, &target, &candidate, &operation, &details)?;
            tx.commit().await?;
            actor.check_session(&self.store).await?;
            Ok(RecordingLyricReview {
                candidate: details,
                review_stamp,
            })
        })
        .await
    }

    pub async fn retain_recording_lyric_identity(
        &self,
        actor: &Actor,
        recording: &str,
        request: RetainRecordingLyricIdentity,
    ) -> Result<RecordingLyricIdentity> {
        workflow("lyric_identity_retain", super::LOOKUP_BUDGET, async {
            if request.review_stamp.len() != 82 || !request.review_stamp.starts_with("lyric-identity-v1:") {
                return Err(Error::Invalid("Use a current lyric identity review".into()));
            }
            let target = RecordingTarget::capture(
                &mut *self.store.pool.acquire().await?, actor, recording,
            ).await?;
            let (operation, details) = self
                .lyric_candidate(actor, recording, &target, &request.candidate)
                .await?;
            let mut tx = self.store.pool.begin_with("BEGIN IMMEDIATE").await?;
            target.recheck(&mut tx, actor, recording).await?;
            operation.recheck(&mut tx).await?;
            if request.review_stamp != stamp(actor, recording, &target, &request.candidate, &operation, &details)? {
                return Err(Error::Conflict("The lyric candidate or recording changed. Review it again.".into()));
            }
            let reference = ProviderRecordingRef {
                id: details.recording.id.clone(),
                revision: details.recording.revision.clone(),
            };
            let existing: Option<(String, i64)> = sqlx::query_as("SELECT id,revision FROM recording_lyric_identities WHERE user_id=? AND recording_id=? AND installation_id=? AND provider_recording_id=? AND provider_revision IS ?")
                .bind(actor.id())
                .bind(recording)
                .bind(&request.candidate.installation_id)
                .bind(&reference.id)
                .bind(&reference.revision)
                .fetch_optional(&mut *tx)
                .await?;
            let (id, revision) = if let Some(existing) = existing {
                existing
            } else {
                let count: i64 = sqlx::query_scalar("SELECT count(*) FROM recording_lyric_identities WHERE user_id=? AND recording_id=?")
                    .bind(actor.id())
                    .bind(recording)
                    .fetch_one(&mut *tx)
                    .await?;
                if count >= 8 {
                    return Err(Error::Conflict("Remove a lyric identity before retaining another".into()));
                }
                let id = Uuid::new_v4().to_string();
                let encoded = serde_json::to_string(&details).map_err(|_| Error::Internal("Reviewed lyric metadata could not be encoded".into()))?;
                sqlx::query("INSERT INTO recording_lyric_identities(id,user_id,recording_id,installation_id,provider_recording_id,provider_revision,reviewed_metadata) VALUES (?,?,?,?,?,?,?)")
                    .bind(&id)
                    .bind(actor.id())
                    .bind(recording)
                    .bind(&request.candidate.installation_id)
                    .bind(&reference.id)
                    .bind(&reference.revision)
                    .bind(encoded)
                    .execute(&mut *tx)
                    .await?;
                (id, 1)
            };
            target.recheck(&mut tx, actor, recording).await?;
            operation.recheck(&mut tx).await?;
            let label: String = sqlx::query_scalar("SELECT json_extract(manifest, '$.name') FROM plugin_installations WHERE id=?")
                .bind(&request.candidate.installation_id)
                .fetch_one(&mut *tx)
                .await?;
            tx.commit().await?;
            actor.check_session(&self.store).await?;
            Ok(RecordingLyricIdentity {
                selectable: true,
                selector: RecordingLyricSelector::Reviewed {
                    identity_id: id,
                    identity_revision: revision,
                },
                installation_id: request.candidate.installation_id,
                installation_revision: request.candidate.installation_revision,
                provider_recording: reference,
                label,
            })
        })
        .await
    }

    async fn lyric_candidate(
        &self,
        actor: &Actor,
        recording: &str,
        target: &RecordingTarget,
        candidate: &ProviderCandidate,
    ) -> Result<(Operation, RecordingMetadata)> {
        validate_installation(&candidate.installation_id, candidate.installation_revision)?;
        let request = RecordingRequest {
            recording_id: candidate.provider_recording_id.clone(),
        };
        request
            .validate()
            .map_err(|_| Error::Invalid("Use a valid lyric provider recording identity".into()))?;
        let operation = self
            .plugins
            .operation(
                actor,
                &candidate.installation_id,
                candidate.installation_revision,
                REQUIRED,
            )
            .await?;
        let details = self
            .lyric_exchange(
                actor,
                recording,
                target,
                &operation,
                operation.lyric_metadata_checked(&request, || {
                    self.check_lyric_target(actor, recording, target, &operation)
                }),
            )
            .await?;
        Ok((operation, details))
    }
}

fn stamp(
    actor: &Actor,
    recording: &str,
    target: &RecordingTarget,
    candidate: &ProviderCandidate,
    operation: &Operation,
    metadata: &RecordingMetadata,
) -> Result<String> {
    let value = serde_json::to_vec(&(
        "lyric-identity-v1",
        actor.id(),
        actor.session_binding(),
        recording,
        target,
        candidate,
        &operation.provider,
        &operation.service,
        operation.service_revision,
        metadata,
    ))
    .map_err(|_| Error::Internal("Lyric review could not be encoded".into()))?;
    Ok(format!("lyric-identity-v1:{:x}", Sha256::digest(value)))
}
