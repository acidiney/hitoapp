use super::{
    ArtistAssociation, ArtistCreditRole, ArtistReviewContext, AssociateRecordingArtist,
    CreditMemberReviewContext, REQUEST_BUDGET, associations,
    credit_target::CreditTarget,
    policy,
    provider::{self, Connection},
    review::Authority,
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
            self.confirm_review(actor, Authority::Whole(context), provider_artist_id),
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

    #[tracing::instrument(level = "info", skip_all, fields(operation = "credit_member_review_confirm", request_id = %Uuid::new_v4()))]
    pub async fn confirm_credit_member_review(
        &self,
        actor: &Actor,
        context: &CreditMemberReviewContext,
        provider_artist_id: &str,
    ) -> Result<ArtistAssociation> {
        let started = Instant::now();
        let result = tokio::time::timeout(
            REQUEST_BUDGET,
            self.confirm_review(actor, Authority::Member(context), provider_artist_id),
        )
        .await
        .unwrap_or(Err(Error::Upstream(
            "Credit member review confirmation exceeded the time limit",
        )));
        tracing::info!(
            outcome = result
                .as_ref()
                .map(|_| "success")
                .unwrap_or_else(|error| error.code()),
            elapsed_ms = started.elapsed().as_millis() as u64,
            "Credit member review confirmation completed"
        );
        result
    }

    async fn confirm_review(
        &self,
        actor: &Actor,
        authority: Authority<'_>,
        provider_artist_id: &str,
    ) -> Result<ArtistAssociation> {
        let _admission = self.admit(actor.id())?;
        let (account, recording, library, artist, source, observation, policy_revision, target) =
            match authority {
                Authority::Whole(context) => (
                    &context.account_id,
                    &context.recording_id,
                    &context.library_id,
                    &context.artist_id,
                    &context.source_id,
                    &context.observation_id,
                    context.policy_revision,
                    CreditTarget::WholeCredit,
                ),
                Authority::Member(context) => (
                    &context.account_id,
                    &context.recording_id,
                    &context.library_id,
                    &context.artist_id,
                    &context.source_id,
                    &context.observation_id,
                    context.policy_revision,
                    CreditTarget::NamedMember {
                        artist_id: context.artist_id.clone(),
                        role: context.role,
                    },
                ),
            };
        if account != actor.id() {
            return Err(Error::NotFound);
        }
        actor.check_session(&self.store).await?;
        provider::validate_artist_id(authority.provider(), provider_artist_id)?;
        let connection = Connection::open(self, actor, authority.provider()).await?;
        let mut tx = self.store.pool.begin_with("BEGIN IMMEDIATE").await?;
        if !authority
            .recheck_on(&mut tx, actor, Some(&connection))
            .await?
        {
            return Err(policy::changed());
        }
        let request = AssociateRecordingArtist {
            artist_id: artist.clone(),
            source_id: source.clone(),
            observation_id: observation.clone(),
            role: ArtistCreditRole::RecordingArtist,
            provider: authority.provider().clone(),
            provider_artist_id: provider_artist_id.into(),
            policy_revision,
        };
        let association =
            associations::retain_target(&mut tx, actor, recording, library, &request, &target)
                .await?;
        tx.commit().await?;
        Ok(association)
    }
}
