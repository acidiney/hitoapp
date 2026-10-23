use super::{
    ArtistCandidateSearchAvailability, ArtistReviewContext, REQUEST_BUDGET, provider::Connection,
    review,
};
use crate::{Actor, Catalog, Error, Result};
use hito_plugin_sdk::ArtistSearchRequest;
use std::{future::Future, time::Instant};
use uuid::Uuid;

impl Catalog {
    #[tracing::instrument(level = "debug", skip_all, fields(operation = "artist_candidate_search", request_id = %Uuid::new_v4()))]
    pub async fn search_artist_candidates_until(
        &self,
        actor: &Actor,
        context: &ArtistReviewContext,
        request: ArtistSearchRequest,
        cancelled: impl Future<Output = ()> + Send,
    ) -> Result<ArtistCandidateSearchAvailability> {
        let started = Instant::now();
        let deadline = tokio::time::Instant::now() + REQUEST_BUDGET;
        tokio::pin!(cancelled);
        let result = tokio::select! {
            biased;
            _ = &mut cancelled => Ok(ArtistCandidateSearchAvailability::Cancelled),
            result = tokio::time::timeout_at(deadline, self.search_artist_candidates(actor, context, request)) => {
                result.unwrap_or(Ok(ArtistCandidateSearchAvailability::Unavailable))
            }
        };
        tracing::debug!(
            outcome = result
                .as_ref()
                .map(ArtistCandidateSearchAvailability::outcome)
                .unwrap_or_else(|error| error.code()),
            item_count = match &result {
                Ok(ArtistCandidateSearchAvailability::Available { page, .. }) => page.items.len(),
                _ => 0,
            },
            elapsed_ms = started.elapsed().as_millis() as u64,
            "Artist candidate search completed"
        );
        result
    }

    async fn search_artist_candidates(
        &self,
        actor: &Actor,
        context: &ArtistReviewContext,
        request: ArtistSearchRequest,
    ) -> Result<ArtistCandidateSearchAvailability> {
        let _admission = self.artist_catalog.admit(actor.id())?;
        request
            .validate()
            .map_err(|_| Error::Invalid("Use a valid artist candidate search".into()))?;
        if !review::recheck(self, actor, context, None).await? {
            return Ok(ArtistCandidateSearchAvailability::Disabled);
        }
        let connection = match Connection::open(self, actor, &context.provider).await {
            Err(Error::NotFound) => return Ok(ArtistCandidateSearchAvailability::Unavailable),
            result => result?,
        };
        if !review::recheck(self, actor, context, Some(&connection)).await? {
            return Ok(ArtistCandidateSearchAvailability::Disabled);
        }
        let supported = connection.discovery_supported().await;
        if !review::recheck(self, actor, context, Some(&connection)).await? {
            return Ok(ArtistCandidateSearchAvailability::Disabled);
        }
        match supported {
            Ok(false) => return Ok(ArtistCandidateSearchAvailability::Unsupported),
            Err(Error::Upstream(_)) => return Ok(ArtistCandidateSearchAvailability::Unavailable),
            result => {
                result?;
            }
        }
        let result = connection.search(&request).await;
        if !review::recheck(self, actor, context, Some(&connection)).await? {
            return Ok(ArtistCandidateSearchAvailability::Disabled);
        }
        let page = match result {
            Err(Error::Upstream(_)) => return Ok(ArtistCandidateSearchAvailability::Unavailable),
            result => result?,
        };
        page.validate_for(&request)
            .map_err(|_| Error::Upstream("The provider returned invalid artist candidates"))?;
        Ok(ArtistCandidateSearchAvailability::Available {
            context: Box::new(context.clone()),
            page,
        })
    }
}
