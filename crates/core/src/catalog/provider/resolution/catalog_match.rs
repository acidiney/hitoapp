use super::{
    admission::Caller, catalog_decision::CatalogDecision, cursor::Candidates, fallback::Basis,
    matching::MusicalRecording, provider::Subject,
};
use crate::{Actor, Catalog, Result, catalog::provider::adoption::AcceptedRecording};

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
    pub accepted: AcceptedRecording,
}

pub(crate) enum CatalogAudio {
    Found(Box<FoundAudio>),
    NoMatch,
    InsufficientEvidence,
    NoProviders,
    Unavailable,
}

pub(crate) struct AudioPermit(#[allow(dead_code)] Caller);

impl Catalog {
    pub(crate) async fn admit_catalog_audio(&self, actor: &Actor) -> Result<AudioPermit> {
        let mut caller = self.resolution_callers.enter(actor.id())?;
        caller.wait().await?;
        Ok(AudioPermit(caller))
    }

    pub(crate) async fn find_audio_for_catalog_track(
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
        let (found, basis) = match candidates.next(self, actor, &subject).await? {
            Some(candidate) => (Some(candidate), Basis::Matched),
            None => loop {
                let Some((index, candidate)) = candidates
                    .next_by_provider_rank(self, actor, &subject)
                    .await?
                else {
                    break (None, Basis::Matched);
                };
                let mut connection = self.store.pool.acquire().await?;
                actor.check_session_on(&mut connection).await?;
                candidate.operation.recheck(&mut connection).await?;
                let occupied: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM provider_items WHERE installation_id = ? AND provider_recording_id = ? AND retired = 0)")
                    .bind(&candidate.installation).bind(&candidate.details.id).fetch_one(&mut *connection).await?;
                if occupied {
                    tracing::debug!(
                        event = "catalog_audio_candidate_skipped",
                        outcome = "already_bound",
                        candidate_rank = index,
                        "Catalog audio candidate is already bound"
                    );
                    continue;
                }
                break (Some(candidate), Basis::ProviderRank { index });
            },
        };
        tracing::debug!(
            event = "catalog_audio_search_finished",
            connections = candidates.connection_count,
            matched = found.is_some(),
            basis = basis.as_str(),
            candidate_rank = basis.rank_index(),
            provider_unavailable = candidates.provider_unavailable,
            "Catalog track audio search finished"
        );
        Ok(match found {
            Some(candidate) => {
                let decision = match basis {
                    Basis::Matched => None,
                    Basis::ProviderRank { .. } => {
                        Some(CatalogDecision::new(&musical, &candidate, basis)?)
                    }
                };
                CatalogAudio::Found(Box::new(FoundAudio {
                    installation_id: candidate.installation,
                    installation_revision: candidate.installation_revision,
                    provider_recording_id: candidate.details.id.clone(),
                    accepted: AcceptedRecording {
                        decision,
                        sources: candidate
                            .sources
                            .items
                            .iter()
                            .map(|source| (source.id.clone(), source.ready))
                            .collect(),
                        details: candidate.details,
                    },
                }))
            }
            None if candidates.provider_unavailable => CatalogAudio::Unavailable,
            None => CatalogAudio::NoMatch,
        })
    }
}
