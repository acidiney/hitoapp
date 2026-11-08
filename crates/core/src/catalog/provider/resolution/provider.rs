use super::{
    authority,
    fallback::{Basis, FALLBACK_POLICY},
    matching::{METADATA_POLICY, MusicalRecording, POLICY},
};
use crate::{Actor, Catalog, Error, Result, plugins::Operation};
use hito_plugin_sdk::{
    Capability, ProviderRecording, RecordingMetadata, RecordingRequest, SearchRequest, SourcePage,
};
use std::{
    collections::{BTreeSet, HashMap},
    iter::Take,
    sync::Arc,
    vec::IntoIter,
};

const DETAIL_LIMIT: usize = 3;

pub(super) struct Subject<'a> {
    pub(super) musical: &'a MusicalRecording,
    pub(super) recording: Option<&'a str>,
    pub(super) binding: Option<&'a str>,
}

enum CandidateEvidence {
    Legacy,
    Musical(Box<RecordingMetadata>),
}

pub(super) struct Candidate {
    pub installation: String,
    pub installation_revision: i64,
    pub operation: Arc<Operation>,
    pub details: ProviderRecording,
    pub sources: SourcePage,
    evidence: CandidateEvidence,
}

impl Candidate {
    pub(super) fn acceptable(&self, target: &MusicalRecording, basis: Basis) -> bool {
        match basis {
            Basis::Matched => self.rejection(target).is_none(),
            Basis::ProviderRank { .. } => self.technical_rejection().is_none(),
        }
    }

    fn rejection(&self, target: &MusicalRecording) -> Option<&'static str> {
        self.technical_rejection()
            .or_else(|| self.matching_rejection(target))
    }

    fn technical_rejection(&self) -> Option<&'static str> {
        if self.details.validate().is_err() {
            return Some("invalid_details");
        }
        if self.sources.validate().is_err() {
            return Some("invalid_sources");
        }
        if !self.sources.items.iter().any(|source| source.ready) {
            return Some("no_ready_source");
        }
        match &self.evidence {
            CandidateEvidence::Legacy => self
                .operation
                .metadata_approved()
                .then_some("metadata_evidence_required"),
            CandidateEvidence::Musical(metadata) => {
                let Some(witnesses) = metadata.source_ids.as_ref() else {
                    return Some("missing_source_witnesses");
                };
                if !self.operation.metadata_approved() {
                    return Some("metadata_not_approved");
                }
                if metadata.validate().is_err() {
                    return Some("invalid_metadata");
                }
                if metadata.recording != self.details {
                    return Some("metadata_details_mismatch");
                }
                if witnesses.iter().collect::<BTreeSet<_>>()
                    != self
                        .sources
                        .items
                        .iter()
                        .map(|source| &source.id)
                        .collect::<BTreeSet<_>>()
                {
                    return Some("source_witness_mismatch");
                }
                None
            }
        }
    }

    fn technical_rejection_for(&self, summary: &str) -> Option<&'static str> {
        if self.details.id != summary {
            return Some("detail_identity_mismatch");
        }
        self.technical_rejection()
    }

    fn matching_rejection(&self, target: &MusicalRecording) -> Option<&'static str> {
        match &self.evidence {
            CandidateEvidence::Legacy => {
                (!target.accepts(&self.details)).then_some("legacy_musical_mismatch")
            }
            CandidateEvidence::Musical(metadata) => target.metadata_rejection(metadata),
        }
    }

    pub(super) fn policy(&self, basis: Basis) -> &'static str {
        match (basis, &self.evidence) {
            (Basis::ProviderRank { .. }, _) => FALLBACK_POLICY,
            (Basis::Matched, CandidateEvidence::Legacy) => POLICY,
            (Basis::Matched, CandidateEvidence::Musical(_)) => METADATA_POLICY,
        }
    }

    pub(super) fn digest(&self) -> Result<String> {
        match &self.evidence {
            CandidateEvidence::Legacy => authority::digest(&(&self.details, &self.sources)),
            CandidateEvidence::Musical(metadata) => {
                authority::digest(&(&self.details, &self.sources, metadata.as_ref()))
            }
        }
    }

    pub(super) fn musical_metadata(&self) -> Result<Option<String>> {
        match &self.evidence {
            CandidateEvidence::Legacy => Ok(None),
            CandidateEvidence::Musical(metadata) => serde_json::to_string(metadata.as_ref())
                .map(Some)
                .map_err(|_| {
                    Error::Internal("Musical source evidence could not be encoded".into())
                }),
        }
    }
}

pub(super) struct Page {
    pub(super) provider_unavailable: bool,
    installation: String,
    revision: i64,
    operation: Arc<Operation>,
    summaries: Take<IntoIter<ProviderRecording>>,
    ranked: IntoIter<(usize, ProviderRecording)>,
    hydrated: HashMap<String, Option<Candidate>>,
}

impl Catalog {
    pub(super) async fn search_resolution_connection(
        &self,
        actor: &Actor,
        musical: &MusicalRecording,
        installation: String,
        revision: i64,
        query: &str,
    ) -> Result<Page> {
        let operation = self
            .plugins
            .operation(
                actor,
                &installation,
                revision,
                &[Capability::Search, Capability::Sources],
            )
            .await?;
        let mut page = operation
            .search(&SearchRequest {
                query: query.into(),
                cursor: None,
                limit: 20,
            })
            .await?;
        {
            let mut connection = self.store.pool.acquire().await?;
            actor.check_session_on(&mut connection).await?;
            operation.recheck(&mut connection).await?;
        }
        let returned_count = page.items.len();
        let ranked = page
            .items
            .iter()
            .take(DETAIL_LIMIT)
            .cloned()
            .enumerate()
            .collect::<Vec<_>>();
        page.items.retain(|candidate| {
            if operation.metadata_approved() {
                musical.plausible_metadata_summary(candidate)
            } else {
                musical.plausible(candidate)
            }
        });
        tracing::debug!(
            event = "source_resolution_search_candidates",
            returned_count,
            plausible_count = page.items.len(),
            detail_limit = DETAIL_LIMIT,
            "Source resolution search candidates filtered"
        );
        Ok(Page {
            provider_unavailable: false,
            installation,
            revision,
            operation: Arc::new(operation),
            summaries: page.items.into_iter().take(DETAIL_LIMIT),
            ranked: ranked.into_iter(),
            hydrated: HashMap::new(),
        })
    }
}

impl Page {
    pub(super) async fn next(
        &mut self,
        catalog: &Catalog,
        actor: &Actor,
        subject: &Subject<'_>,
    ) -> Result<Option<Candidate>> {
        let (recording, musical) = (subject.recording, subject.musical);
        loop {
            let Some(summary) = self.summaries.next() else {
                return Ok(None);
            };
            let occupied = self.occupied(catalog, actor, subject, &summary).await?;
            if occupied {
                tracing::debug!(
                    event = "source_resolution_candidate_skipped",
                    outcome = "already_bound",
                    "Source candidate belongs to another recording"
                );
                continue;
            }
            let request = RecordingRequest {
                recording_id: summary.id.clone(),
            };
            let candidate = match self.hydrate(catalog, actor, recording, &request).await {
                Ok(candidate) => candidate,
                Err(Error::Unauthorized) => return Err(Error::Unauthorized),
                Err(error) => {
                    self.provider_unavailable |=
                        matches!(&error, Error::Upstream(_) | Error::ProviderBusy(_));
                    tracing::debug!(
                        event = "source_resolution_candidate_failed",
                        error_kind = error.code(),
                        "Source resolution candidate exchange failed"
                    );
                    self.hydrated.insert(summary.id.clone(), None);
                    continue;
                }
            };
            let technical = candidate.technical_rejection_for(&summary.id);
            let rejection = technical.or_else(|| candidate.matching_rejection(musical));
            if let Some(reason) = rejection {
                tracing::debug!(
                    event = "source_resolution_candidate_rejected",
                    reason,
                    "Source resolution candidate rejected"
                );
                self.hydrated
                    .insert(summary.id.clone(), technical.is_none().then_some(candidate));
                continue;
            }
            return Ok(Some(candidate));
        }
    }

    pub(super) async fn next_by_provider_rank(
        &mut self,
        catalog: &Catalog,
        actor: &Actor,
        subject: &Subject<'_>,
    ) -> Result<Option<(usize, Candidate)>> {
        loop {
            let Some((rank, summary)) = self.ranked.next() else {
                return Ok(None);
            };
            if self.occupied(catalog, actor, subject, &summary).await? {
                continue;
            }
            let candidate = match self.hydrated.remove(&summary.id) {
                Some(Some(candidate)) => candidate,
                Some(None) => continue,
                None => {
                    let request = RecordingRequest {
                        recording_id: summary.id.clone(),
                    };
                    match self
                        .hydrate(catalog, actor, subject.recording, &request)
                        .await
                    {
                        Ok(candidate)
                            if candidate.technical_rejection_for(&summary.id).is_none() =>
                        {
                            candidate
                        }
                        Ok(_) => continue,
                        Err(Error::Unauthorized) => return Err(Error::Unauthorized),
                        Err(error) => {
                            self.provider_unavailable |=
                                matches!(&error, Error::Upstream(_) | Error::ProviderBusy(_));
                            tracing::debug!(
                                event = "source_resolution_candidate_failed",
                                error_kind = error.code(),
                                "Source resolution candidate exchange failed"
                            );
                            continue;
                        }
                    }
                }
            };
            return Ok(Some((rank, candidate)));
        }
    }

    async fn occupied(
        &self,
        catalog: &Catalog,
        actor: &Actor,
        subject: &Subject<'_>,
        summary: &ProviderRecording,
    ) -> Result<bool> {
        let mut connection = catalog.store.pool.acquire().await?;
        authority::check(&mut connection, actor, subject.recording).await?;
        self.operation.recheck(&mut connection).await?;
        Ok(sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(SELECT 1 FROM provider_items candidate
            JOIN provider_items primary_item ON primary_item.id = ?
            WHERE candidate.installation_id = ? AND candidate.provider_recording_id = ?
                AND candidate.retired = 0 AND candidate.recording_id != primary_item.recording_id)",
        )
        .bind(subject.binding)
        .bind(&self.installation)
        .bind(&summary.id)
        .fetch_one(&mut *connection)
        .await?)
    }

    async fn hydrate(
        &self,
        catalog: &Catalog,
        actor: &Actor,
        recording: Option<&str>,
        request: &RecordingRequest,
    ) -> Result<Candidate> {
        let details = self.operation.recording_details(request).await;
        self.recheck(catalog, actor, recording).await?;
        let details = details?;
        let sources = self.operation.sources(request).await;
        self.recheck(catalog, actor, recording).await?;
        let sources = sources?;
        let evidence = if self.operation.metadata_approved() {
            let metadata = self.operation.recording_metadata(request).await;
            self.recheck(catalog, actor, recording).await?;
            CandidateEvidence::Musical(Box::new(metadata?))
        } else {
            CandidateEvidence::Legacy
        };
        Ok(Candidate {
            installation: self.installation.clone(),
            installation_revision: self.revision,
            operation: Arc::clone(&self.operation),
            details,
            sources,
            evidence,
        })
    }

    async fn recheck(
        &self,
        catalog: &Catalog,
        actor: &Actor,
        recording: Option<&str>,
    ) -> Result<()> {
        let mut connection = catalog.store.pool.acquire().await?;
        authority::check(&mut connection, actor, recording).await?;
        self.operation.recheck(&mut connection).await
    }
}
