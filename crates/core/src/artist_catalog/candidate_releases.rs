use super::{
    ArtistCandidateReleasesAvailability, ArtistReviewContext, REQUEST_BUDGET,
    provider::{self, Connection},
    review,
};
use crate::{Actor, ArtistCatalog, Error, Result};
use hito_plugin_sdk::ArtistDiscographyRequest;
use std::{future::Future, time::Instant};
use uuid::Uuid;

const KNOWN_RELEASE_TITLES: u16 = 3;

impl ArtistCatalog {
    #[tracing::instrument(level = "debug", skip_all, fields(operation = "artist_candidate_releases", request_id = %Uuid::new_v4()))]
    pub async fn known_releases_of_artist_candidate_until(
        &self,
        actor: &Actor,
        context: &ArtistReviewContext,
        provider_artist_id: &str,
        cancelled: impl Future<Output = ()> + Send,
    ) -> Result<ArtistCandidateReleasesAvailability> {
        let started = Instant::now();
        let deadline = tokio::time::Instant::now() + REQUEST_BUDGET;
        tokio::pin!(cancelled);
        let result = tokio::select! {
            biased;
            _ = &mut cancelled => Ok(ArtistCandidateReleasesAvailability::Cancelled),
            result = tokio::time::timeout_at(deadline, self.known_releases_of_artist_candidate(actor, context, provider_artist_id)) => {
                result.unwrap_or(Ok(ArtistCandidateReleasesAvailability::Unavailable))
            }
        };
        tracing::debug!(
            outcome = result
                .as_ref()
                .map(ArtistCandidateReleasesAvailability::outcome)
                .unwrap_or_else(|error| error.code()),
            item_count = match &result {
                Ok(ArtistCandidateReleasesAvailability::Available { titles, .. }) => titles.len(),
                _ => 0,
            },
            elapsed_ms = started.elapsed().as_millis() as u64,
            "Artist candidate releases completed"
        );
        result
    }

    async fn known_releases_of_artist_candidate(
        &self,
        actor: &Actor,
        context: &ArtistReviewContext,
        provider_artist_id: &str,
    ) -> Result<ArtistCandidateReleasesAvailability> {
        let _admission = self.admit(actor.id())?;
        provider::validate_artist_id(&context.provider, provider_artist_id)?;
        if !review::recheck(self, actor, context, None).await? {
            return Ok(ArtistCandidateReleasesAvailability::Disabled);
        }
        let connection = match Connection::open(self, actor, &context.provider).await {
            Err(Error::NotFound) => return Ok(ArtistCandidateReleasesAvailability::Unavailable),
            result => result?,
        };
        if !review::recheck(self, actor, context, Some(&connection)).await? {
            return Ok(ArtistCandidateReleasesAvailability::Disabled);
        }
        let supported = connection.discography_supported().await;
        if !review::recheck(self, actor, context, Some(&connection)).await? {
            return Ok(ArtistCandidateReleasesAvailability::Disabled);
        }
        match supported {
            Ok(false) => return Ok(ArtistCandidateReleasesAvailability::Unsupported),
            Err(Error::Upstream(_)) => return Ok(ArtistCandidateReleasesAvailability::Unavailable),
            result => {
                result?;
            }
        }
        let request = ArtistDiscographyRequest {
            artist_id: provider_artist_id.into(),
            cursor: None,
            snapshot: None,
            limit: KNOWN_RELEASE_TITLES,
            release_type: None,
        };
        let result = connection.discography(&request).await;
        if !review::recheck(self, actor, context, Some(&connection)).await? {
            return Ok(ArtistCandidateReleasesAvailability::Disabled);
        }
        let page = match result {
            Err(Error::Upstream(_)) => return Ok(ArtistCandidateReleasesAvailability::Unavailable),
            result => result?,
        };
        page.validate_for(&request)
            .map_err(|_| Error::Upstream("The provider returned an invalid artist discography"))?;
        Ok(ArtistCandidateReleasesAvailability::Available {
            context: Box::new(context.clone()),
            titles: page
                .items
                .into_iter()
                .take(usize::from(KNOWN_RELEASE_TITLES))
                .map(|group| group.title)
                .collect(),
        })
    }
}
