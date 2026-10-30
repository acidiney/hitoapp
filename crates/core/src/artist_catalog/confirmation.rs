use super::{
    ArtistAssociation, ArtistCreditRole, ArtistReviewContext, AssociateRecordingArtist,
    REQUEST_BUDGET, associations, policy,
    provider::{self, Connection},
    review,
};
use crate::{Actor, ArtistCatalog, Error, Result};
use std::time::Instant;
use uuid::Uuid;

impl ArtistCatalog {
    #[tracing::instrument(level = "info", skip_all, fields(operation = "artist_review_confirm", request_id = %Uuid::new_v4()))]
    pub async fn confirm_artist_review(
        &self,
        actor: &Actor,
        context: &ArtistReviewContext,
        provider_artist_id: &str,
    ) -> Result<ArtistAssociation> {
        let started = Instant::now();
        let result = tokio::time::timeout(
            REQUEST_BUDGET,
            self.confirm_review(actor, context, provider_artist_id),
        )
        .await
        .unwrap_or(Err(Error::Upstream(
            "Artist review confirmation exceeded the time limit",
        )));
        tracing::info!(
            outcome = result
                .as_ref()
                .map(|_| "success")
                .unwrap_or_else(|error| error.code()),
            elapsed_ms = started.elapsed().as_millis() as u64,
            "Artist review confirmation completed"
        );
        result
    }

    async fn confirm_review(
        &self,
        actor: &Actor,
        context: &ArtistReviewContext,
        provider_artist_id: &str,
    ) -> Result<ArtistAssociation> {
        let _admission = self.admit(actor.id())?;
        if context.account_id != actor.id() {
            return Err(Error::NotFound);
        }
        actor.check_session(&self.store).await?;
        provider::validate_artist_id(&context.provider, provider_artist_id)?;
        let connection = Connection::open(self, actor, &context.provider).await?;
        let mut tx = self.store.pool.begin_with("BEGIN IMMEDIATE").await?;
        if !review::recheck_on(&mut tx, actor, context, Some(&connection)).await? {
            return Err(policy::changed());
        }
        let request = AssociateRecordingArtist {
            artist_id: context.artist_id.clone(),
            source_id: context.source_id.clone(),
            observation_id: context.observation_id.clone(),
            role: ArtistCreditRole::RecordingArtist,
            provider: context.provider.clone(),
            provider_artist_id: provider_artist_id.into(),
            policy_revision: context.policy_revision,
        };
        let association = associations::retain(
            &mut tx,
            actor,
            &context.recording_id,
            &context.library_id,
            &request,
        )
        .await?;
        tx.commit().await?;
        Ok(association)
    }
}
