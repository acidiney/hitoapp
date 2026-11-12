use super::{authority, fallback::Basis, matching::MusicalRecording, provider::Candidate};
use crate::{Actor, Result, plugins::Operation};
use sqlx::SqliteConnection;
use std::sync::Arc;
use uuid::Uuid;

pub(crate) struct CatalogDecision {
    policy: &'static str,
    operation: Arc<Operation>,
    target_digest: String,
    candidate_digest: String,
    installation_revision: i64,
    service_revision: i64,
    musical_metadata: Option<String>,
}

impl CatalogDecision {
    pub(super) fn new(
        target: &MusicalRecording,
        candidate: &Candidate,
        basis: Basis,
    ) -> Result<Self> {
        Ok(Self {
            policy: candidate.policy(basis),
            operation: Arc::clone(&candidate.operation),
            target_digest: authority::digest(target)?,
            candidate_digest: candidate.digest()?,
            installation_revision: candidate.installation_revision,
            service_revision: candidate.operation.service_revision,
            musical_metadata: candidate.musical_metadata()?,
        })
    }

    pub(in crate::catalog::provider) async fn recheck(
        &self,
        connection: &mut SqliteConnection,
    ) -> Result<()> {
        self.operation.recheck(connection).await
    }

    pub(in crate::catalog::provider) async fn persist(
        &self,
        connection: &mut SqliteConnection,
        actor: &Actor,
        recording: &str,
        binding: &str,
    ) -> Result<()> {
        let id = Uuid::new_v4().to_string();
        sqlx::query("INSERT INTO source_resolutions(id,user_id,recording_id,target_digest,choice_revision,outcome,completed_at) VALUES (?,?,?,?,0,'ready',unixepoch())")
            .bind(&id).bind(actor.id()).bind(recording).bind(&self.target_digest)
            .execute(&mut *connection).await?;
        sqlx::query("INSERT INTO automatic_source_decisions(resolution_id,binding_id,policy_version,target_digest,candidate_digest,installation_revision,service_revision,musical_metadata) VALUES (?,?,?,?,?,?,?,?)")
            .bind(&id).bind(binding).bind(self.policy).bind(&self.target_digest)
            .bind(&self.candidate_digest).bind(self.installation_revision).bind(self.service_revision)
            .bind(&self.musical_metadata).execute(connection).await?;
        tracing::debug!(
            event = "catalog_audio_decision_saved",
            resolution_id = id,
            policy = self.policy,
            outcome = "ready",
            "Catalog audio decision saved"
        );
        Ok(())
    }
}
