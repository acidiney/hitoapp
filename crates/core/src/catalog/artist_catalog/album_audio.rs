use super::{
    REQUEST_BUDGET,
    album_editions::{AlbumEditionTrackListAvailability, ReadAlbumEditionTrackList},
    editions::EditionRead,
};
use crate::{
    Actor, Catalog, Error, Result,
    catalog::provider::{CatalogAudio, CatalogTrack},
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

#[derive(Serialize, ToSchema)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum AlbumTrackAudio {
    Found {
        installation_id: String,
        installation_revision: i64,
        provider_recording_id: String,
    },
    NoMatch,
    NoProviders,
    Disabled,
    Unsupported,
    Unavailable,
    Cancelled,
    NeedsChoice,
}

impl AlbumTrackAudio {
    pub fn outcome(&self) -> &'static str {
        match self {
            Self::Found { .. } => "found",
            Self::NoMatch => "no_match",
            Self::NoProviders => "no_providers",
            Self::Disabled => "disabled",
            Self::Unsupported => "unsupported",
            Self::Unavailable => "unavailable",
            Self::Cancelled => "cancelled",
            Self::NeedsChoice => "needs_choice",
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

impl Catalog {
    /// Finds playable audio for one catalog-only track of a chosen edition
    /// through the account's approved source providers. The track's evidence
    /// comes from a fresh read of the edition, never from the caller. Nothing
    /// is saved, and a candidate that the automatic source policy does not
    /// accept is reported as no match.
    #[tracing::instrument(level = "debug", skip_all, fields(operation = "album_track_audio", request_id = %Uuid::new_v4()))]
    pub async fn find_album_track_audio_until(
        &self,
        actor: &Actor,
        release: &str,
        request: FindAlbumTrackAudio,
        cancelled: impl Future<Output = ()> + Send,
    ) -> Result<AlbumTrackAudio> {
        let started = Instant::now();
        tokio::pin!(cancelled);
        let result = tokio::select! {
            biased;
            _ = &mut cancelled => Ok(AlbumTrackAudio::Cancelled),
            result = tokio::time::timeout(REQUEST_BUDGET, self.find_album_track_audio(actor, release, request)) => {
                result.unwrap_or(Ok(AlbumTrackAudio::Unavailable))
            }
        };
        tracing::debug!(
            outcome = result
                .as_ref()
                .map(AlbumTrackAudio::outcome)
                .unwrap_or_else(|error| error.code()),
            elapsed_ms = started.elapsed().as_millis() as u64,
            "Album track audio search completed"
        );
        result
    }

    async fn find_album_track_audio(
        &self,
        actor: &Actor,
        release: &str,
        request: FindAlbumTrackAudio,
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
        let edition = match read {
            EditionRead::Available(found) => found.edition,
            EditionRead::Missing(status) => {
                return Ok(AlbumEditionTrackListAvailability::from(status).into());
            }
        };
        let track = catalog_track(&edition, &occurrence).ok_or(Error::NotFound)?;
        Ok(
            match self.find_audio_for_catalog_track(actor, track).await? {
                CatalogAudio::Found(found) => AlbumTrackAudio::Found {
                    installation_id: found.installation_id,
                    installation_revision: found.installation_revision,
                    provider_recording_id: found.provider_recording_id,
                },
                CatalogAudio::NoMatch => AlbumTrackAudio::NoMatch,
                CatalogAudio::NoProviders => AlbumTrackAudio::NoProviders,
                CatalogAudio::Unavailable => AlbumTrackAudio::Unavailable,
            },
        )
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
