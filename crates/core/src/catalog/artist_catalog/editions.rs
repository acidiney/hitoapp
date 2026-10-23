use super::{
    EditionTrackListAvailability, REQUEST_BUDGET, ReadEditionTrackList, ReadReleaseGroupEditions,
    ReleaseGroupEditionsAvailability,
    authority::{ReviewedRead, ReviewedReadState},
    continuation,
};
use crate::{Actor, Catalog, Error, Result};
use hito_plugin_sdk::{
    CatalogEditionOperation, EditionTrackListRequest, ReleaseGroupEditionsRequest,
};
use std::{future::Future, time::Instant};
use uuid::Uuid;

impl Catalog {
    #[tracing::instrument(level = "debug", skip_all, fields(operation = "artist_release_group_editions", request_id = %Uuid::new_v4()))]
    pub async fn recording_artist_release_group_editions_until(
        &self,
        actor: &Actor,
        recording: &str,
        association: &str,
        request: ReadReleaseGroupEditions,
        cancelled: impl Future<Output = ()> + Send,
    ) -> Result<ReleaseGroupEditionsAvailability> {
        let started = Instant::now();
        let deadline = tokio::time::Instant::now() + REQUEST_BUDGET;
        tokio::pin!(cancelled);
        let result = tokio::select! {
            biased;
            _ = &mut cancelled => Ok(ReleaseGroupEditionsAvailability::Cancelled),
            result = tokio::time::timeout_at(deadline, self.read_artist_editions(actor, recording, association, request)) => {
                result.unwrap_or(Ok(ReleaseGroupEditionsAvailability::Unavailable))
            }
        };
        tracing::debug!(
            outcome = result
                .as_ref()
                .map(ReleaseGroupEditionsAvailability::outcome)
                .unwrap_or_else(|error| error.code()),
            item_count = match &result {
                Ok(ReleaseGroupEditionsAvailability::Available { page, .. }) => page.items.len(),
                _ => 0,
            },
            elapsed_ms = started.elapsed().as_millis() as u64,
            "Artist release-group editions read completed"
        );
        result
    }

    async fn read_artist_editions(
        &self,
        actor: &Actor,
        recording: &str,
        association: &str,
        request: ReadReleaseGroupEditions,
    ) -> Result<ReleaseGroupEditionsAvailability> {
        let first = ReleaseGroupEditionsRequest {
            release_group_id: request.release_group_id,
            cursor: None,
            snapshot: None,
            limit: request.limit,
        };
        first
            .validate()
            .map_err(|_| Error::Invalid("Use a valid release-group edition request".into()))?;
        if request.continuation.as_ref().is_some_and(|token| {
            token.is_empty() || token.len() > continuation::MAX_TOKEN_BYTES || !token.is_ascii()
        }) {
            return Err(Error::Invalid(
                "Use a valid catalog continuation or start again".into(),
            ));
        }
        let read = match ReviewedRead::capture(self, actor, recording, association).await? {
            ReviewedReadState::Available(read) => read,
            ReviewedReadState::Disabled => return Ok(ReleaseGroupEditionsAvailability::Disabled),
            ReviewedReadState::Unavailable => {
                return Ok(ReleaseGroupEditionsAvailability::Unavailable);
            }
        };
        let binding = read.continuation_binding()?;
        let provider_request = match request.continuation {
            Some(token) => self.artist_catalog.continuations.open_editions(
                &token,
                binding,
                &first.release_group_id,
                first.limit,
            )?,
            None => first,
        };
        let supported = read
            .connection
            .edition_supported(CatalogEditionOperation::ReleaseGroupEditions)
            .await;
        read.recheck().await?;
        match supported {
            Ok(false) => return Ok(ReleaseGroupEditionsAvailability::Unsupported),
            Err(Error::Upstream(_)) => return Ok(ReleaseGroupEditionsAvailability::Unavailable),
            result => {
                result?;
            }
        }
        let result = read.connection.editions(&provider_request).await;
        read.recheck().await?;
        let page = match result {
            Err(Error::Upstream(_)) => return Ok(ReleaseGroupEditionsAvailability::Unavailable),
            result => result?,
        };
        page.validate_for(&provider_request)
            .map_err(|_| Error::Upstream("The provider returned invalid release-group editions"))?;
        let continuation = page
            .next_cursor
            .as_ref()
            .map(|cursor| {
                self.artist_catalog.continuations.seal_editions(
                    binding,
                    ReleaseGroupEditionsRequest {
                        release_group_id: provider_request.release_group_id.clone(),
                        cursor: Some(cursor.clone()),
                        snapshot: page.snapshot.clone(),
                        limit: provider_request.limit,
                    },
                )
            })
            .transpose()?;
        Ok(ReleaseGroupEditionsAvailability::Available {
            association_id: read.association.id,
            page,
            continuation,
        })
    }

    #[tracing::instrument(level = "debug", skip_all, fields(operation = "artist_edition_track_list", request_id = %Uuid::new_v4()))]
    pub async fn recording_artist_edition_track_list_until(
        &self,
        actor: &Actor,
        recording: &str,
        association: &str,
        request: ReadEditionTrackList,
        cancelled: impl Future<Output = ()> + Send,
    ) -> Result<EditionTrackListAvailability> {
        let started = Instant::now();
        let deadline = tokio::time::Instant::now() + REQUEST_BUDGET;
        tokio::pin!(cancelled);
        let result = tokio::select! {
            biased;
            _ = &mut cancelled => Ok(EditionTrackListAvailability::Cancelled),
            result = tokio::time::timeout_at(deadline, self.read_artist_edition_tracks(actor, recording, association, request)) => {
                result.unwrap_or(Ok(EditionTrackListAvailability::Unavailable))
            }
        };
        tracing::debug!(
            outcome = result
                .as_ref()
                .map(EditionTrackListAvailability::outcome)
                .unwrap_or_else(|error| error.code()),
            medium_count = match &result {
                Ok(EditionTrackListAvailability::Available { edition, .. }) => edition.media.len(),
                _ => 0,
            },
            track_count = match &result {
                Ok(EditionTrackListAvailability::Available { edition, .. }) => {
                    edition
                        .media
                        .iter()
                        .map(|medium| medium.tracks.len())
                        .sum::<usize>()
                }
                _ => 0,
            },
            elapsed_ms = started.elapsed().as_millis() as u64,
            "Artist edition track list read completed"
        );
        result
    }

    async fn read_artist_edition_tracks(
        &self,
        actor: &Actor,
        recording: &str,
        association: &str,
        request: ReadEditionTrackList,
    ) -> Result<EditionTrackListAvailability> {
        let provider_request = EditionTrackListRequest {
            release_group_id: request.release_group_id,
            edition_id: request.edition_id,
        };
        provider_request
            .validate()
            .map_err(|_| Error::Invalid("Use a valid edition track-list request".into()))?;
        let read = match ReviewedRead::capture(self, actor, recording, association).await? {
            ReviewedReadState::Available(read) => read,
            ReviewedReadState::Disabled => return Ok(EditionTrackListAvailability::Disabled),
            ReviewedReadState::Unavailable => return Ok(EditionTrackListAvailability::Unavailable),
        };
        let supported = read
            .connection
            .edition_supported(CatalogEditionOperation::EditionTrackList)
            .await;
        read.recheck().await?;
        match supported {
            Ok(false) => return Ok(EditionTrackListAvailability::Unsupported),
            Err(Error::Upstream(_)) => return Ok(EditionTrackListAvailability::Unavailable),
            result => {
                result?;
            }
        }
        let result = read.connection.edition_tracks(&provider_request).await;
        read.recheck().await?;
        let edition = match result {
            Err(Error::Upstream(_)) => return Ok(EditionTrackListAvailability::Unavailable),
            result => result?,
        };
        edition
            .validate_for(&provider_request)
            .map_err(|_| Error::Upstream("The provider returned an invalid edition track list"))?;
        Ok(EditionTrackListAvailability::Available {
            association_id: read.association.id,
            edition,
        })
    }
}
