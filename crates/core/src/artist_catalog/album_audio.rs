use super::{
    REQUEST_BUDGET, ReadEditionTrackList,
    album_editions::{AlbumEditionTrackListAvailability, ReadAlbumEditionTrackList},
    editions::EditionRead,
};
use crate::{
    Actor, AdoptProviderRecording, ArtistCatalog, Error, Recording, Result,
    catalog::{AudioPermit, CatalogAudio, CatalogTrack},
};
use hito_plugin_sdk::{CatalogCreditMember, CatalogEditionTrackList, CatalogTrackKind};
use serde::{Deserialize, Serialize};
use std::{future::Future, time::Instant};
use utoipa::ToSchema;
use uuid::Uuid;

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct FindAlbumTrackAudio {
    pub recording_id: String,
    pub association_id: String,
    pub release_group_id: String,
    pub edition_id: String,
    pub occurrence_id: String,
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct FindArtistTrackAudio {
    pub release_group_id: String,
    pub edition_id: String,
    pub occurrence_id: String,
}

#[derive(Serialize, ToSchema)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum AlbumTrackAudio {
    Saved { recording: Box<Recording> },
    NoMatch,
    InsufficientEvidence,
    NoProviders,
    Busy,
    Changed,
    Disabled,
    Unsupported,
    Unavailable,
    Cancelled,
    NeedsChoice,
    SearchOff,
}

impl AlbumTrackAudio {
    pub fn outcome(&self) -> &'static str {
        match self {
            Self::Saved { .. } => "saved",
            Self::NoMatch => "no_match",
            Self::InsufficientEvidence => "insufficient_evidence",
            Self::NoProviders => "no_providers",
            Self::Busy => "busy",
            Self::Changed => "changed",
            Self::Disabled => "disabled",
            Self::Unsupported => "unsupported",
            Self::Unavailable => "unavailable",
            Self::Cancelled => "cancelled",
            Self::NeedsChoice => "needs_choice",
            Self::SearchOff => "search_off",
        }
    }
}

impl From<AlbumEditionTrackListAvailability> for AlbumTrackAudio {
    fn from(status: AlbumEditionTrackListAvailability) -> Self {
        match status {
            AlbumEditionTrackListAvailability::Disabled => Self::Disabled,
            AlbumEditionTrackListAvailability::Unsupported => Self::Unsupported,
            AlbumEditionTrackListAvailability::Cancelled => Self::Cancelled,
            AlbumEditionTrackListAvailability::NeedsChoice => Self::NeedsChoice,
            AlbumEditionTrackListAvailability::Unavailable
            | AlbumEditionTrackListAvailability::Available { .. } => Self::Unavailable,
        }
    }
}

impl ArtistCatalog {
    #[tracing::instrument(level = "debug", skip_all, fields(operation = "album_track_audio", request_id = %Uuid::new_v4()))]
    pub async fn find_album_track_audio_until(
        &self,
        actor: &Actor,
        release: &str,
        request: FindAlbumTrackAudio,
        cancelled: impl Future<Output = ()> + Send,
    ) -> Result<AlbumTrackAudio> {
        self.bounded_track_audio(actor, cancelled, |permit| async move {
            self.find_album_track_audio(actor, release, request, &permit)
                .await
        })
        .await
    }

    #[tracing::instrument(level = "debug", skip_all, fields(operation = "artist_track_audio", request_id = %Uuid::new_v4()))]
    pub async fn find_artist_track_audio_until(
        &self,
        actor: &Actor,
        recording: &str,
        association: &str,
        request: FindArtistTrackAudio,
        cancelled: impl Future<Output = ()> + Send,
    ) -> Result<AlbumTrackAudio> {
        self.bounded_track_audio(actor, cancelled, |permit| async move {
            let read = self
                .read_edition(
                    actor,
                    recording,
                    association,
                    ReadEditionTrackList {
                        release_group_id: request.release_group_id,
                        edition_id: request.edition_id,
                    },
                )
                .await?;
            self.audio_for_occurrence(actor, read, &request.occurrence_id, &permit)
                .await
        })
        .await
    }

    async fn when_search_is_on<Search, Searching>(
        &self,
        actor: &Actor,
        search: Search,
        permit: AudioPermit,
    ) -> Result<AlbumTrackAudio>
    where
        Search: FnOnce(AudioPermit) -> Searching,
        Searching: Future<Output = Result<AlbumTrackAudio>>,
    {
        if !self.automatic_service_search_allowed(actor).await? {
            return Ok(AlbumTrackAudio::SearchOff);
        }
        search(permit).await
    }

    async fn bounded_track_audio<Search, Searching>(
        &self,
        actor: &Actor,
        cancelled: impl Future<Output = ()> + Send,
        search: Search,
    ) -> Result<AlbumTrackAudio>
    where
        Search: FnOnce(AudioPermit) -> Searching,
        Searching: Future<Output = Result<AlbumTrackAudio>>,
    {
        let started = Instant::now();
        let permit = match self.catalog.admit_catalog_audio(actor).await {
            Ok(permit) => permit,
            Err(Error::Conflict(_)) => return Ok(AlbumTrackAudio::Busy),
            Err(error) => return Err(error),
        };
        tokio::pin!(cancelled);
        let result = tokio::select! {
            biased;
            _ = &mut cancelled => Ok(AlbumTrackAudio::Cancelled),
            result = tokio::time::timeout(REQUEST_BUDGET, self.when_search_is_on(actor, search, permit)) => {
                result.unwrap_or(Ok(AlbumTrackAudio::Unavailable))
            }
        };
        tracing::debug!(
            outcome = result
                .as_ref()
                .map(AlbumTrackAudio::outcome)
                .unwrap_or_else(|error| error.code()),
            elapsed_ms = started.elapsed().as_millis() as u64,
            "Catalog track audio search completed"
        );
        result
    }

    async fn find_album_track_audio(
        &self,
        actor: &Actor,
        release: &str,
        request: FindAlbumTrackAudio,
        permit: &AudioPermit,
    ) -> Result<AlbumTrackAudio> {
        let occurrence = request.occurrence_id;
        let (_, read) = self
            .read_album_edition_tracks(
                actor,
                release,
                ReadAlbumEditionTrackList {
                    recording_id: request.recording_id,
                    association_id: request.association_id,
                    release_group_id: request.release_group_id,
                    edition_id: request.edition_id,
                },
            )
            .await?;
        self.audio_for_occurrence(actor, read, &occurrence, permit)
            .await
    }

    async fn audio_for_occurrence(
        &self,
        actor: &Actor,
        read: EditionRead,
        occurrence: &str,
        permit: &AudioPermit,
    ) -> Result<AlbumTrackAudio> {
        let (provider, edition) = match read {
            EditionRead::Available(found) => (found.association.provider, found.edition),
            EditionRead::Missing(status) => {
                return Ok(AlbumEditionTrackListAvailability::from(status).into());
            }
        };
        let track = catalog_track(&edition, occurrence).ok_or(Error::NotFound)?;
        if let Some(linked) = self
            .linked_catalog_tracks(actor, &provider, &edition.id, Some(occurrence))
            .await?
            .pop()
        {
            return Ok(AlbumTrackAudio::Saved {
                recording: Box::new(linked.recording),
            });
        }
        let found = match self
            .catalog
            .find_audio_for_catalog_track(actor, track, permit)
            .await?
        {
            CatalogAudio::Found(found) => found,
            CatalogAudio::NoMatch => return Ok(AlbumTrackAudio::NoMatch),
            CatalogAudio::InsufficientEvidence => {
                return Ok(AlbumTrackAudio::InsufficientEvidence);
            }
            CatalogAudio::NoProviders => return Ok(AlbumTrackAudio::NoProviders),
            CatalogAudio::Unavailable => return Ok(AlbumTrackAudio::Unavailable),
        };
        let saved = self
            .catalog
            .adopt_accepted(
                actor,
                &found.installation_id,
                AdoptProviderRecording {
                    installation_revision: found.installation_revision,
                    provider_recording_id: found.provider_recording_id,
                },
                Some(&found.accepted),
            )
            .await;
        Ok(match saved {
            Ok(Some(recording)) => {
                self.link_catalog_track(actor, &provider, &edition.id, occurrence, &recording.id)
                    .await?;
                AlbumTrackAudio::Saved {
                    recording: Box::new(recording),
                }
            }
            Ok(None) => AlbumTrackAudio::Changed,
            Err(Error::Upstream(_) | Error::ProviderBusy(_)) => AlbumTrackAudio::Unavailable,
            Err(error) => return Err(error),
        })
    }
}

fn catalog_track(edition: &CatalogEditionTrackList, occurrence: &str) -> Option<CatalogTrack> {
    let track = edition
        .media
        .iter()
        .flat_map(|medium| &medium.tracks)
        .find(|track| track.id == occurrence && track.kind == CatalogTrackKind::Audio)?;
    let artist = credit_text(track.artist_credit.as_deref())
        .or_else(|| credit_text(edition.artist_credit.as_deref()))
        .unwrap_or_default();
    Some(CatalogTrack {
        title: track.title.clone(),
        artist,
        duration_ms: track
            .duration_ms
            .and_then(|duration| i64::try_from(duration).ok())
            .unwrap_or(0),
        album: Some(edition.title.clone()),
    })
}

fn credit_text(credit: Option<&[CatalogCreditMember]>) -> Option<String> {
    let text: String = credit?
        .iter()
        .map(|member| format!("{}{}", member.credited_name, member.join_phrase))
        .collect();
    let text = text.trim();
    (!text.is_empty()).then(|| text.to_owned())
}
