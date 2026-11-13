use super::{
    ArtistCandidateSearchAvailability, ArtistReviewContext,
    CreditMemberCandidateSearchAvailability, CreditMemberReviewContext, REQUEST_BUDGET,
    provider::Connection, review::Authority,
};
use crate::{Actor, ArtistCatalog, Error, Result, plugins::CatalogDiscoveryResponse};
use hito_plugin_sdk::{ArtistSearchPage, ArtistSearchRequest};
use std::{future::Future, time::Instant};
use uuid::Uuid;

enum SearchResult {
    Available(ArtistSearchPage),
    Disabled,
    Unsupported,
    Unavailable,
}

impl ArtistCatalog {
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
            result = tokio::time::timeout_at(deadline, async {
                self.search_candidates(actor, Authority::Whole(context), request).await.map(|found| match found {
                    SearchResult::Available(page) => ArtistCandidateSearchAvailability::Available {
                        context: Box::new(context.clone()), page,
                    },
                    SearchResult::Disabled => ArtistCandidateSearchAvailability::Disabled,
                    SearchResult::Unsupported => ArtistCandidateSearchAvailability::Unsupported,
                    SearchResult::Unavailable => ArtistCandidateSearchAvailability::Unavailable,
                })
            }) => {
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

    #[tracing::instrument(level = "debug", skip_all, fields(operation = "credit_member_candidate_search", request_id = %Uuid::new_v4()))]
    pub async fn search_credit_member_candidates_until(
        &self,
        actor: &Actor,
        context: &CreditMemberReviewContext,
        request: ArtistSearchRequest,
        cancelled: impl Future<Output = ()> + Send,
    ) -> Result<CreditMemberCandidateSearchAvailability> {
        let started = Instant::now();
        let deadline = tokio::time::Instant::now() + REQUEST_BUDGET;
        tokio::pin!(cancelled);
        let result = tokio::select! {
            biased;
            _ = &mut cancelled => Ok(CreditMemberCandidateSearchAvailability::Cancelled),
            result = tokio::time::timeout_at(deadline, async {
                self.search_candidates(actor, Authority::Member(context), request).await.map(|found| match found {
                    SearchResult::Available(page) => CreditMemberCandidateSearchAvailability::Available {
                        context: Box::new(context.clone()), page,
                    },
                    SearchResult::Disabled => CreditMemberCandidateSearchAvailability::Disabled,
                    SearchResult::Unsupported => CreditMemberCandidateSearchAvailability::Unsupported,
                    SearchResult::Unavailable => CreditMemberCandidateSearchAvailability::Unavailable,
                })
            }) => {
                result.unwrap_or(Ok(CreditMemberCandidateSearchAvailability::Unavailable))
            }
        };
        tracing::debug!(
            outcome = result
                .as_ref()
                .map(CreditMemberCandidateSearchAvailability::outcome)
                .unwrap_or_else(|error| error.code()),
            item_count = match &result {
                Ok(CreditMemberCandidateSearchAvailability::Available { page, .. }) =>
                    page.items.len(),
                _ => 0,
            },
            elapsed_ms = started.elapsed().as_millis() as u64,
            "Credit member candidate search completed"
        );
        result
    }

    async fn search_candidates(
        &self,
        actor: &Actor,
        authority: Authority<'_>,
        request: ArtistSearchRequest,
    ) -> Result<SearchResult> {
        let _admission = self.admit(actor.id())?;
        request
            .validate()
            .map_err(|_| Error::Invalid("Use a valid artist candidate search".into()))?;
        if !authority.recheck(self, actor, None).await? {
            return Ok(SearchResult::Disabled);
        }
        let connection = match Connection::open(self, actor, authority.provider()).await {
            Err(Error::NotFound) => return Ok(SearchResult::Unavailable),
            result => result?,
        };
        if !authority.recheck(self, actor, Some(&connection)).await? {
            return Ok(SearchResult::Disabled);
        }
        let CatalogDiscoveryResponse {
            result: supported,
            observation: descriptor_observation,
        } = connection.discovery_supported(authority.is_member()).await;
        if !authority.recheck(self, actor, Some(&connection)).await? {
            return Ok(SearchResult::Disabled);
        }
        let supported = match (&connection, descriptor_observation.as_ref()) {
            (Connection::Http(operation), Some(observation)) => {
                supported.map_err(|error| operation.with_credential_context(error, observation))
            }
            _ => supported,
        };
        match supported {
            Ok(false) => return Ok(SearchResult::Unsupported),
            Err(Error::Upstream(_)) => return Ok(SearchResult::Unavailable),
            result => {
                result?;
            }
        }
        let CatalogDiscoveryResponse {
            result,
            observation: search_observation,
        } = connection.search(&request, authority.is_member()).await;
        if !authority.recheck(self, actor, Some(&connection)).await? {
            return Ok(SearchResult::Disabled);
        }
        let result = match (&connection, search_observation.as_ref()) {
            (Connection::Http(operation), Some(observation)) => {
                result.map_err(|error| operation.with_credential_context(error, observation))
            }
            _ => result,
        };
        let page = match result {
            Err(Error::Upstream(_)) => return Ok(SearchResult::Unavailable),
            result => result?,
        };
        page.validate_for(&request)
            .map_err(|_| Error::Upstream("The provider returned invalid artist candidates"))?;
        if let (Connection::Http(operation), Some(observation)) =
            (&connection, search_observation.as_ref())
        {
            operation.credentials_verified(observation);
        }
        Ok(SearchResult::Available(page))
    }
}
