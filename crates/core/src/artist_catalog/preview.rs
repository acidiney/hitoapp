use super::{
    ArtistCandidatePreviewAvailability, ArtistReviewContext, REQUEST_BUDGET,
    provider::{self, Connection},
    review,
};
use crate::{Actor, ArtistCatalog, Error, Result};
use hito_plugin_sdk::ArtistProfileRequest;
use std::{future::Future, time::Instant};
use uuid::Uuid;

impl ArtistCatalog {
    #[tracing::instrument(level = "debug", skip_all, fields(operation = "artist_candidate_preview", request_id = %Uuid::new_v4()))]
    pub async fn preview_artist_candidate_until(
        &self,
        actor: &Actor,
        context: &ArtistReviewContext,
        provider_artist_id: &str,
        cancelled: impl Future<Output = ()> + Send,
    ) -> Result<ArtistCandidatePreviewAvailability> {
        let started = Instant::now();
        let deadline = tokio::time::Instant::now() + REQUEST_BUDGET;
        tokio::pin!(cancelled);
        let result = tokio::select! {
            biased;
            _ = &mut cancelled => Ok(ArtistCandidatePreviewAvailability::Cancelled),
            result = tokio::time::timeout_at(deadline, self.preview_artist_candidate(actor, context, provider_artist_id)) => {
                result.unwrap_or(Ok(ArtistCandidatePreviewAvailability::Unavailable))
            }
        };
        tracing::debug!(
            outcome = result
                .as_ref()
                .map(ArtistCandidatePreviewAvailability::outcome)
                .unwrap_or_else(|error| error.code()),
            elapsed_ms = started.elapsed().as_millis() as u64,
            "Artist candidate preview completed"
        );
        result
    }

    async fn preview_artist_candidate(
        &self,
        actor: &Actor,
        context: &ArtistReviewContext,
        provider_artist_id: &str,
    ) -> Result<ArtistCandidatePreviewAvailability> {
        let _admission = self.admit(actor.id())?;
        provider::validate_artist_id(&context.provider, provider_artist_id)?;
        if !review::recheck(self, actor, context, None).await? {
            return Ok(ArtistCandidatePreviewAvailability::Disabled);
        }
        let connection = match Connection::open(self, actor, &context.provider).await {
            Err(Error::NotFound) => return Ok(ArtistCandidatePreviewAvailability::Unavailable),
            result => result?,
        };
        if !review::recheck(self, actor, context, Some(&connection)).await? {
            return Ok(ArtistCandidatePreviewAvailability::Disabled);
        }
        let supported = connection.supported().await;
        if !review::recheck(self, actor, context, Some(&connection)).await? {
            return Ok(ArtistCandidatePreviewAvailability::Disabled);
        }
        match supported {
            Ok(false) => return Ok(ArtistCandidatePreviewAvailability::Unsupported),
            Err(Error::Upstream(_)) => return Ok(ArtistCandidatePreviewAvailability::Unavailable),
            result => {
                result?;
            }
        }
        let request = ArtistProfileRequest {
            artist_id: provider_artist_id.into(),
        };
        let result = connection.profile(&request).await;
        if !review::recheck(self, actor, context, Some(&connection)).await? {
            return Ok(ArtistCandidatePreviewAvailability::Disabled);
        }
        let profile = match result {
            Err(Error::Upstream(_)) => return Ok(ArtistCandidatePreviewAvailability::Unavailable),
            result => result?,
        };
        profile
            .validate_for(&request)
            .map_err(|_| Error::Upstream("The provider returned an invalid artist profile"))?;
        Ok(ArtistCandidatePreviewAvailability::Available {
            context: Box::new(context.clone()),
            profile,
        })
    }
}
