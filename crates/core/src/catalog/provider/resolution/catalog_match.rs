use super::{cursor::Candidates, matching::MusicalRecording, provider::Subject};
use crate::{Actor, Catalog, Result};

/// What the catalog says about one track: the only evidence a search may use.
pub(crate) struct CatalogTrack {
    pub title: String,
    pub artist: String,
    pub duration_ms: i64,
    pub album: Option<String>,
}

pub(crate) struct FoundAudio {
    pub installation_id: String,
    pub installation_revision: i64,
    pub provider_recording_id: String,
}

pub(crate) enum CatalogAudio {
    Found(FoundAudio),
    NoMatch,
    NoProviders,
    Unavailable,
}

impl Catalog {
    /// Searches the account's approved search-and-source connections for a
    /// recording that satisfies the automatic source policy for this track.
    /// Nothing is saved; an uncertain candidate is never returned.
    pub(crate) async fn find_audio_for_catalog_track(
        &self,
        actor: &Actor,
        track: CatalogTrack,
    ) -> Result<CatalogAudio> {
        let musical = MusicalRecording {
            title: track.title,
            artist: track.artist,
            duration_ms: track.duration_ms,
            album: track.album,
        };
        if musical.query().is_none() {
            return Ok(CatalogAudio::NoMatch);
        }
        let mut candidates = Candidates::new(self, actor, &musical).await?;
        if candidates.connection_count == 0 {
            return Ok(CatalogAudio::NoProviders);
        }
        let subject = Subject {
            musical: &musical,
            recording: None,
            binding: None,
        };
        let found = candidates.next(self, actor, &subject).await?;
        tracing::debug!(
            event = "catalog_audio_search_finished",
            connections = candidates.connection_count,
            matched = found.is_some(),
            provider_unavailable = candidates.provider_unavailable,
            "Catalog track audio search finished"
        );
        Ok(match found {
            Some(candidate) => CatalogAudio::Found(FoundAudio {
                installation_id: candidate.installation,
                installation_revision: candidate.installation_revision,
                provider_recording_id: candidate.details.id,
            }),
            None if candidates.provider_unavailable => CatalogAudio::Unavailable,
            None => CatalogAudio::NoMatch,
        })
    }
}
