use super::{
    ArtistDiscographyAvailability, REQUEST_BUDGET, ReadArtistDiscography,
    authority::{ReviewedRead, ReviewedReadState},
    continuation,
};
use crate::{Actor, ArtistCatalog, Error, Result};
use hito_plugin_sdk::{ArtistDiscographyRequest, MAX_CATALOG_PAGE_ITEMS};
use std::{future::Future, time::Instant};
use uuid::Uuid;

impl ArtistCatalog {
    #[tracing::instrument(level = "debug", skip_all, fields(operation = "artist_discography", request_id = %Uuid::new_v4()))]
    pub async fn recording_artist_discography_until(
        &self,
        actor: &Actor,
        recording: &str,
        association: &str,
        request: ReadArtistDiscography,
        cancelled: impl Future<Output = ()> + Send,
    ) -> Result<ArtistDiscographyAvailability> {
        let started = Instant::now();
        let deadline = tokio::time::Instant::now() + REQUEST_BUDGET;
        tokio::pin!(cancelled);
        let result = tokio::select! {
            biased;
            _ = &mut cancelled => Ok(ArtistDiscographyAvailability::Cancelled),
            result = tokio::time::timeout_at(deadline, self.read_artist_discography(actor, recording, association, request)) => {
                result.unwrap_or(Ok(ArtistDiscographyAvailability::Unavailable))
            }
        };
        tracing::debug!(
            outcome = result
                .as_ref()
                .map(ArtistDiscographyAvailability::outcome)
                .unwrap_or_else(|error| error.code()),
            item_count = match &result {
                Ok(ArtistDiscographyAvailability::Available { page, .. }) => page.items.len(),
                _ => 0,
            },
            elapsed_ms = started.elapsed().as_millis() as u64,
            "Artist discography read completed"
        );
        result
    }

    async fn read_artist_discography(
        &self,
        actor: &Actor,
        recording: &str,
        association: &str,
        request: ReadArtistDiscography,
    ) -> Result<ArtistDiscographyAvailability> {
        if !(1..=MAX_CATALOG_PAGE_ITEMS).contains(&request.limit)
            || request.continuation.as_ref().is_some_and(|token| {
                token.is_empty() || token.len() > continuation::MAX_TOKEN_BYTES || !token.is_ascii()
            })
        {
            return Err(Error::Invalid(
                "Use a valid catalog page limit and continuation".into(),
            ));
        }
        let read = match ReviewedRead::capture(self, actor, recording, association).await? {
            ReviewedReadState::Available(read) => read,
            ReviewedReadState::Disabled => return Ok(ArtistDiscographyAvailability::Disabled),
            ReviewedReadState::Unavailable => {
                return Ok(ArtistDiscographyAvailability::Unavailable);
            }
        };
        let binding = read.continuation_binding()?;
        let provider_request = match request.continuation {
            Some(token) => self.continuations.open(&token, binding, request.limit)?,
            None => ArtistDiscographyRequest {
                artist_id: read.association.provider_artist_id.clone(),
                cursor: None,
                snapshot: None,
                limit: request.limit,
            },
        };
        if provider_request.artist_id != read.association.provider_artist_id {
            return Err(Error::Invalid(
                "Use a valid catalog continuation or start again".into(),
            ));
        }
        let supported = read.connection.discography_supported().await;
        read.recheck().await?;
        match supported {
            Ok(false) => return Ok(ArtistDiscographyAvailability::Unsupported),
            Err(Error::Upstream(_)) => return Ok(ArtistDiscographyAvailability::Unavailable),
            result => {
                result?;
            }
        }
        let result = read.connection.discography(&provider_request).await;
        read.recheck().await?;
        let page = match result {
            Err(Error::Upstream(_)) => return Ok(ArtistDiscographyAvailability::Unavailable),
            result => result?,
        };
        page.validate_for(&provider_request)
            .map_err(|_| Error::Upstream("The provider returned an invalid artist discography"))?;
        let continuation = page
            .next_cursor
            .as_ref()
            .map(|cursor| {
                self.continuations.seal(
                    binding,
                    ArtistDiscographyRequest {
                        artist_id: read.association.provider_artist_id.clone(),
                        cursor: Some(cursor.clone()),
                        snapshot: page.snapshot.clone(),
                        limit: provider_request.limit,
                    },
                )
            })
            .transpose()?;
        Ok(ArtistDiscographyAvailability::Available {
            association_id: read.association.id,
            page,
            continuation,
        })
    }
}
