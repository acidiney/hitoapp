use super::{admission::Caller, cursor::Candidates, matching::MusicalRecording, provider::Subject};
use crate::{Actor, Catalog, Result, catalog::provider::adoption::AcceptedRecording};

/// What the catalog says about one track: the only evidence a search may use.
pub(in crate::catalog) struct CatalogTrack {
    pub title: String,
    pub artist: String,
    pub duration_ms: i64,
    pub album: Option<String>,
}

pub(in crate::catalog) struct FoundAudio {
    pub installation_id: String,
    pub installation_revision: i64,
    pub provider_recording_id: String,
    pub accepted: AcceptedRecording,
}

pub(in crate::catalog) enum CatalogAudio {
    Found(Box<FoundAudio>),
    NoMatch,
    InsufficientEvidence,
    NoProviders,
    Unavailable,
}

/// Holds the account's source-resolution capacity while a catalog track is
/// being matched and saved.
pub(in crate::catalog) struct AudioPermit(#[allow(dead_code)] Caller);

impl Catalog {
    /// Takes the same per-account and global capacity as source resolution.
    /// A busy answer is a conflict.
    pub(in crate::catalog) async fn admit_catalog_audio(
        &self,
        actor: &Actor,
    ) -> Result<AudioPermit> {
        let mut caller = self.resolution_callers.enter(actor.id())?;
        caller.wait().await?;
        Ok(AudioPermit(caller))
    }

    /// Searches the account's approved search-and-source connections for a
    /// recording that satisfies the automatic source policy for this track.
    /// Nothing is saved; an uncertain candidate is never returned.
    pub(in crate::catalog) async fn find_audio_for_catalog_track(
        &self,
        actor: &Actor,
        track: CatalogTrack,
        _permit: &AudioPermit,
    ) -> Result<CatalogAudio> {
        let musical = MusicalRecording {
            title: track.title,
            artist: track.artist,
            duration_ms: track.duration_ms,
            album: track.album,
        };
        if musical.query().is_none() {
            return Ok(CatalogAudio::InsufficientEvidence);
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
            Some(candidate) => CatalogAudio::Found(Box::new(FoundAudio {
                installation_id: candidate.installation,
                installation_revision: candidate.installation_revision,
                provider_recording_id: candidate.details.id.clone(),
                accepted: AcceptedRecording {
                    sources: candidate
                        .sources
                        .items
                        .iter()
                        .map(|source| (source.id.clone(), source.ready))
                        .collect(),
                    details: candidate.details,
                },
            })),
            None if candidates.provider_unavailable => CatalogAudio::Unavailable,
            None => CatalogAudio::NoMatch,
        })
    }
}
