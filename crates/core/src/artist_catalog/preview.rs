use super::{
    ArtistCandidatePreviewAvailability, ArtistReviewContext,
    CreditMemberCandidatePreviewAvailability, CreditMemberReviewContext, REQUEST_BUDGET,
    provider::{self, Connection},
    review::Authority,
};
use crate::{Actor, ArtistCatalog, Error, Result, plugins::CatalogDiscoveryResponse};
use hito_plugin_sdk::{ArtistProfileRequest, CatalogArtistProfile};
use std::{future::Future, time::Instant};
use uuid::Uuid;

enum PreviewResult {
    Available(CatalogArtistProfile),
    Disabled,
    Unsupported,
    Unavailable,
}

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
            result = tokio::time::timeout_at(deadline, async {
                self.preview_candidate(actor, Authority::Whole(context), provider_artist_id).await.map(|found| match found {
                    PreviewResult::Available(profile) => ArtistCandidatePreviewAvailability::Available {
                        context: Box::new(context.clone()), profile,
                    },
                    PreviewResult::Disabled => ArtistCandidatePreviewAvailability::Disabled,
                    PreviewResult::Unsupported => ArtistCandidatePreviewAvailability::Unsupported,
                    PreviewResult::Unavailable => ArtistCandidatePreviewAvailability::Unavailable,
                })
            }) => {
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

    #[tracing::instrument(level = "debug", skip_all, fields(operation = "credit_member_candidate_preview", request_id = %Uuid::new_v4()))]
    pub async fn preview_credit_member_candidate_until(
        &self,
        actor: &Actor,
        context: &CreditMemberReviewContext,
        provider_artist_id: &str,
        cancelled: impl Future<Output = ()> + Send,
    ) -> Result<CreditMemberCandidatePreviewAvailability> {
        let started = Instant::now();
        let deadline = tokio::time::Instant::now() + REQUEST_BUDGET;
        tokio::pin!(cancelled);
        let result = tokio::select! {
            biased;
            _ = &mut cancelled => Ok(CreditMemberCandidatePreviewAvailability::Cancelled),
            result = tokio::time::timeout_at(deadline, async {
                self.preview_candidate(actor, Authority::Member(context), provider_artist_id).await.map(|found| match found {
                    PreviewResult::Available(profile) => CreditMemberCandidatePreviewAvailability::Available {
                        context: Box::new(context.clone()), profile,
                    },
                    PreviewResult::Disabled => CreditMemberCandidatePreviewAvailability::Disabled,
                    PreviewResult::Unsupported => CreditMemberCandidatePreviewAvailability::Unsupported,
                    PreviewResult::Unavailable => CreditMemberCandidatePreviewAvailability::Unavailable,
                })
            }) => {
                result.unwrap_or(Ok(CreditMemberCandidatePreviewAvailability::Unavailable))
            }
        };
        tracing::debug!(
            outcome = result
                .as_ref()
                .map(CreditMemberCandidatePreviewAvailability::outcome)
                .unwrap_or_else(|error| error.code()),
            elapsed_ms = started.elapsed().as_millis() as u64,
            "Credit member candidate preview completed"
        );
        result
    }

    async fn preview_candidate(
        &self,
        actor: &Actor,
        authority: Authority<'_>,
        provider_artist_id: &str,
    ) -> Result<PreviewResult> {
        let _admission = self.admit(actor.id())?;
        provider::validate_artist_id(authority.provider(), provider_artist_id)?;
        if !authority.recheck(self, actor, None).await? {
            return Ok(PreviewResult::Disabled);
        }
        let connection = match Connection::open(self, actor, authority.provider()).await {
            Err(Error::NotFound) => return Ok(PreviewResult::Unavailable),
            result => result?,
        };
        if !authority.recheck(self, actor, Some(&connection)).await? {
            return Ok(PreviewResult::Disabled);
        }
        let CatalogDiscoveryResponse {
            result: supported,
            observation: descriptor_observation,
        } = connection.preview_supported(authority.is_member()).await;
        if !authority.recheck(self, actor, Some(&connection)).await? {
            return Ok(PreviewResult::Disabled);
        }
        let supported = match (&connection, descriptor_observation.as_ref()) {
            (Connection::Http(operation), Some(observation)) => {
                supported.map_err(|error| operation.with_credential_context(error, observation))
            }
            _ => supported,
        };
        match supported {
            Ok(false) => return Ok(PreviewResult::Unsupported),
            Err(Error::Upstream(_)) => return Ok(PreviewResult::Unavailable),
            result => {
                result?;
            }
        }
        let request = ArtistProfileRequest {
            artist_id: provider_artist_id.into(),
        };
        let CatalogDiscoveryResponse {
            result,
            observation: profile_observation,
        } = connection
            .preview_profile(&request, authority.is_member())
            .await;
        if !authority.recheck(self, actor, Some(&connection)).await? {
            return Ok(PreviewResult::Disabled);
        }
        let result = match (&connection, profile_observation.as_ref()) {
            (Connection::Http(operation), Some(observation)) => {
                result.map_err(|error| operation.with_credential_context(error, observation))
            }
            _ => result,
        };
        let profile = match result {
            Err(Error::Upstream(_)) => return Ok(PreviewResult::Unavailable),
            result => result?,
        };
        profile
            .validate_for(&request)
            .map_err(|_| Error::Upstream("The provider returned an invalid artist profile"))?;
        if let (Connection::Http(operation), Some(observation)) =
            (&connection, profile_observation.as_ref())
        {
            operation.credentials_verified(observation);
        }
        Ok(PreviewResult::Available(profile))
    }
}
