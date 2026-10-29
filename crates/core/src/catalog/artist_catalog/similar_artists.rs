use super::{
    MetadataProvider, REQUEST_BUDGET,
    authority::{ReviewedRead, ReviewedReadState},
    identity::groups::chosen_groups_by_musicbrainz_id,
    policy,
    provider::{Connection, validate_builtin_id},
};
use crate::{Actor, Catalog, Error, Result};
use hito_plugin_sdk::{ArtistSimilarityPage, ArtistSimilarityRequest, MAX_SIMILAR_ARTISTS};
use serde::{Deserialize, Serialize};
use std::{future::Future, time::Instant};
use utoipa::ToSchema;
use uuid::Uuid;

const MAX_ANSWER_ATTEMPTS: usize = 3;
const MAX_PROBES: usize = 8;

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ReadSimilarArtists {
    pub limit: u16,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct SimilarArtistView {
    pub provider_artist_id: String,
    pub name: String,
    pub score: Option<f64>,
    /// The account's chosen artist with exactly this provider identity, if any.
    pub chosen_artist_id: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum SimilarArtistsAvailability {
    Available {
        association_id: String,
        provider: MetadataProvider,
        artists: Vec<SimilarArtistView>,
    },
    Disabled,
    Unsupported,
    Unavailable,
    Cancelled,
}

impl SimilarArtistsAvailability {
    fn outcome(&self) -> &'static str {
        match self {
            Self::Available { .. } => "available",
            Self::Disabled => "disabled",
            Self::Unsupported => "unsupported",
            Self::Unavailable => "unavailable",
            Self::Cancelled => "cancelled",
        }
    }
}

impl Catalog {
    /// Similar artists for a chosen artist identity, from the account's chosen similarity
    /// providers in priority order. Nothing is adopted, imported or merged.
    #[tracing::instrument(level = "debug", skip_all, fields(operation = "similar_artists", request_id = %Uuid::new_v4()))]
    pub async fn recording_similar_artists_until(
        &self,
        actor: &Actor,
        recording: &str,
        association: &str,
        request: ReadSimilarArtists,
        cancelled: impl Future<Output = ()> + Send,
    ) -> Result<SimilarArtistsAvailability> {
        let started = Instant::now();
        let deadline = tokio::time::Instant::now() + REQUEST_BUDGET;
        tokio::pin!(cancelled);
        let result = tokio::select! {
            biased;
            _ = &mut cancelled => Ok(SimilarArtistsAvailability::Cancelled),
            result = tokio::time::timeout_at(deadline, self.read_similar_artists(actor, recording, association, request)) => {
                result.unwrap_or(Ok(SimilarArtistsAvailability::Unavailable))
            }
        };
        tracing::debug!(
            outcome = result
                .as_ref()
                .map(SimilarArtistsAvailability::outcome)
                .unwrap_or_else(|error| error.code()),
            item_count = match &result {
                Ok(SimilarArtistsAvailability::Available { artists, .. }) => artists.len(),
                _ => 0,
            },
            elapsed_ms = started.elapsed().as_millis() as u64,
            "Similar artists read completed"
        );
        result
    }

    async fn read_similar_artists(
        &self,
        actor: &Actor,
        recording: &str,
        association: &str,
        request: ReadSimilarArtists,
    ) -> Result<SimilarArtistsAvailability> {
        if !(1..=MAX_SIMILAR_ARTISTS).contains(&request.limit) {
            return Err(Error::Invalid(
                "Use a similar-artist limit between one and 25".into(),
            ));
        }
        let read = match ReviewedRead::capture(self, actor, recording, association).await? {
            ReviewedReadState::Available(read) => read,
            ReviewedReadState::Disabled => return Ok(SimilarArtistsAvailability::Disabled),
            ReviewedReadState::Unavailable => return Ok(SimilarArtistsAvailability::Unavailable),
        };
        let artist_id = read.association.provider_artist_id.clone();
        if read.association.provider != MetadataProvider::BuiltinMusicBrainz
            || validate_builtin_id(&artist_id).is_err()
        {
            return Ok(SimilarArtistsAvailability::Unsupported);
        }
        let provider_request = ArtistSimilarityRequest {
            artist_id,
            limit: request.limit,
        };
        let settings = {
            let mut connection = self.store.pool.acquire().await?;
            policy::read(&mut connection, actor).await?
        };
        let mut capable = false;
        let mut probe_failed = false;
        let mut attempts = 0;
        for provider in settings
            .providers
            .iter()
            .filter(|provider| matches!(provider, MetadataProvider::HttpPlugin { .. }))
            .take(MAX_PROBES)
        {
            if attempts == MAX_ANSWER_ATTEMPTS {
                break;
            }
            let connection = match Connection::open(self, actor, provider).await {
                Err(Error::NotFound) => continue,
                result => result?,
            };
            match connection.similarity_supported().await {
                Ok(true) => capable = true,
                Ok(false) => {
                    read.recheck().await?;
                    continue;
                }
                Err(Error::Upstream(_)) => {
                    probe_failed = true;
                    read.recheck().await?;
                    continue;
                }
                Err(error) => return Err(error),
            }
            attempts += 1;
            let page = match connection.similar_artists(&provider_request).await {
                Ok(page) if canonical(&page) => page,
                Ok(_) | Err(Error::Upstream(_)) => {
                    read.recheck().await?;
                    continue;
                }
                Err(error) => return Err(error),
            };
            read.recheck().await?;
            let mut tx = self.store.pool.begin().await?;
            connection.recheck(&mut tx).await?;
            let ids: Vec<String> = page
                .items
                .iter()
                .map(|item| item.artist.id.clone())
                .collect();
            let chosen = chosen_groups_by_musicbrainz_id(&mut tx, actor, &ids).await?;
            tx.commit().await?;
            let artists = page
                .items
                .into_iter()
                .map(|item| SimilarArtistView {
                    chosen_artist_id: chosen.get(&item.artist.id).cloned(),
                    provider_artist_id: item.artist.id,
                    name: item.artist.name,
                    score: item.score,
                })
                .collect();
            return Ok(SimilarArtistsAvailability::Available {
                association_id: read.association.id.clone(),
                provider: provider.clone(),
                artists,
            });
        }
        Ok(if capable || probe_failed {
            SimilarArtistsAvailability::Unavailable
        } else {
            SimilarArtistsAvailability::Unsupported
        })
    }
}

fn canonical(page: &ArtistSimilarityPage) -> bool {
    page.items
        .iter()
        .all(|item| validate_builtin_id(&item.artist.id).is_ok())
}

#[cfg(test)]
#[path = "tests/similar_lookup_work.rs"]
mod lookup_work;
