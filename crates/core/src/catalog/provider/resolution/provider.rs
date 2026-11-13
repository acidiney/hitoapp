use super::{
    authority::{self, Traversal},
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

#[derive(Clone, Copy)]
pub(super) struct RetainedSource<'a> {
    pub(super) recording: &'a str,
    pub(super) target_digest: &'a str,
    pub(super) binding: &'a str,
}

pub(super) struct Subject<'a> {
    pub(super) musical: &'a MusicalRecording,
    pub(super) source: Option<RetainedSource<'a>>,
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
        subject: &Subject<'_>,
        installation: String,
        revision: i64,
        query: &str,
    ) -> Result<Traversal<Page>> {
        let operation = self
            .plugins
            .operation(
                actor,
                &installation,
                revision,
                &[Capability::Search, Capability::Sources],
            )
            .await?;
        if matches!(
            check_exchange_authority(self, actor, subject, &operation).await?,
            Traversal::Superseded
        ) {
            return Ok(Traversal::Superseded);
        }
        let observation = operation.credential_observation();
        let result = operation
            .search(&SearchRequest {
                query: query.into(),
                cursor: None,
                limit: 20,
            })
            .await;
        let mut tx = self.store.pool.begin().await?;
        if matches!(
            authority::current(&mut tx, actor, subject.source).await?,
            Traversal::Superseded
        ) {
            tx.commit().await?;
            return Ok(Traversal::Superseded);
        }
        #[cfg(feature = "test-support")]
        tracing::debug!(
            target: "hito_core::authority_snapshot_test",
            point = "source_search_postcheck",
            snapshot_current = true,
            transaction_open = <sqlx::sqlite::SqliteTransactionManager as sqlx::TransactionManager>::get_transaction_depth(&*tx) > 0,
            "Owned authority snapshot checkpoint"
        );
        let mut generation = match operation.recheck(&mut tx).await {
            Ok(()) => Ok(()),
            Err(error @ (Error::NotFound | Error::Conflict(_))) => Err(error),
            Err(error) => return Err(error),
        };
        tx.commit().await?;
        if generation.is_ok() {
            generation = match operation.check_current().await {
                Ok(()) => Ok(()),
                Err(error @ (Error::NotFound | Error::Conflict(_))) => Err(error),
                Err(error) => return Err(error),
            };
        }
        {
            let mut connection = self.store.pool.acquire().await?;
            if matches!(
                authority::current(&mut connection, actor, subject.source).await?,
                Traversal::Superseded
            ) {
                return Ok(Traversal::Superseded);
            }
        }
        let current_source = subject.source.is_some() && generation.is_ok();
        if current_source && result.is_ok() {
            operation.credentials_verified(&observation);
        }
        let mut page = match result {
            Err(error @ Error::PluginCredentialsRequired(_)) if current_source => {
                return Err(operation.with_credential_context(error, &observation));
            }
            result => result?,
        };
        generation?;
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
                subject.musical.plausible_metadata_summary(candidate)
            } else {
                subject.musical.plausible(candidate)
            }
        });
        tracing::debug!(
            event = "source_resolution_search_candidates",
            returned_count,
            plausible_count = page.items.len(),
            detail_limit = DETAIL_LIMIT,
            "Source resolution search candidates filtered"
        );
        Ok(Traversal::Current(Page {
            provider_unavailable: false,
            installation,
            revision,
            operation: Arc::new(operation),
            summaries: page.items.into_iter().take(DETAIL_LIMIT),
            ranked: ranked.into_iter(),
            hydrated: HashMap::new(),
        }))
    }
}

impl Page {
    pub(super) async fn next(
        &mut self,
        catalog: &Catalog,
        actor: &Actor,
        subject: &Subject<'_>,
    ) -> Result<Traversal<Option<Candidate>>> {
        loop {
            let Some(summary) = self.summaries.next() else {
                return Ok(Traversal::Current(None));
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
            let candidate = match self.hydrate(catalog, actor, subject, &request).await {
                Ok(Traversal::Current(candidate)) => candidate,
                Ok(Traversal::Superseded) => return Ok(Traversal::Superseded),
                Err(Error::Unauthorized) => return Err(Error::Unauthorized),
                Err(error) => {
                    self.provider_unavailable |= super::is_provider_unavailable(&error);
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
            let rejection = technical.or_else(|| candidate.matching_rejection(subject.musical));
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
            return Ok(Traversal::Current(Some(candidate)));
        }
    }

    pub(super) async fn next_by_provider_rank(
        &mut self,
        catalog: &Catalog,
        actor: &Actor,
        subject: &Subject<'_>,
    ) -> Result<Traversal<Option<(usize, Candidate)>>> {
        loop {
            let Some((rank, summary)) = self.ranked.next() else {
                return Ok(Traversal::Current(None));
            };
            if self.occupied(catalog, actor, subject, &summary).await? {
                continue;
            }
            let candidate = match self.hydrated.remove(&summary.id) {
                Some(Some(candidate)) => {
                    if matches!(
                        check_exchange_authority(catalog, actor, subject, &self.operation).await?,
                        Traversal::Superseded
                    ) {
                        return Ok(Traversal::Superseded);
                    }
                    candidate
                }
                Some(None) => continue,
                None => {
                    let request = RecordingRequest {
                        recording_id: summary.id.clone(),
                    };
                    match self.hydrate(catalog, actor, subject, &request).await {
                        Ok(Traversal::Current(candidate))
                            if candidate.technical_rejection_for(&summary.id).is_none() =>
                        {
                            candidate
                        }
                        Ok(Traversal::Current(_)) => continue,
                        Ok(Traversal::Superseded) => return Ok(Traversal::Superseded),
                        Err(Error::Unauthorized) => return Err(Error::Unauthorized),
                        Err(error) => {
                            self.provider_unavailable |= super::is_provider_unavailable(&error);
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
            return Ok(Traversal::Current(Some((rank, candidate))));
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
        authority::check(
            &mut connection,
            actor,
            subject.source.map(|source| source.recording),
        )
        .await?;
        self.operation.recheck(&mut connection).await?;
        Ok(sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(SELECT 1 FROM provider_items candidate
            JOIN provider_items primary_item ON primary_item.id = ?
            WHERE candidate.installation_id = ? AND candidate.provider_recording_id = ?
                AND candidate.retired = 0 AND candidate.recording_id != primary_item.recording_id)",
        )
        .bind(subject.source.map(|source| source.binding))
        .bind(&self.installation)
        .bind(&summary.id)
        .fetch_one(&mut *connection)
        .await?)
    }

    async fn hydrate(
        &self,
        catalog: &Catalog,
        actor: &Actor,
        subject: &Subject<'_>,
        request: &RecordingRequest,
    ) -> Result<Traversal<Candidate>> {
        if matches!(
            check_exchange_authority(catalog, actor, subject, &self.operation).await?,
            Traversal::Superseded
        ) {
            return Ok(Traversal::Superseded);
        }
        let observation = subject
            .source
            .map(|_| self.operation.credential_observation());
        let details = self.operation.recording_details(request).await;
        if matches!(
            check_exchange_authority(catalog, actor, subject, &self.operation).await?,
            Traversal::Superseded
        ) {
            return Ok(Traversal::Superseded);
        }
        let details = match (observation.as_ref(), details) {
            (Some(observation), Ok(details)) => {
                self.operation.credentials_verified(observation);
                details
            }
            (Some(observation), Err(error)) => {
                return Err(self.operation.with_credential_context(error, observation));
            }
            (None, result) => result?,
        };
        let observation = subject
            .source
            .map(|_| self.operation.credential_observation());
        let sources = self.operation.sources(request).await;
        if matches!(
            check_exchange_authority(catalog, actor, subject, &self.operation).await?,
            Traversal::Superseded
        ) {
            return Ok(Traversal::Superseded);
        }
        let sources = match (observation.as_ref(), sources) {
            (Some(observation), Ok(sources)) => {
                self.operation.credentials_verified(observation);
                sources
            }
            (Some(observation), Err(error)) => {
                return Err(self.operation.with_credential_context(error, observation));
            }
            (None, result) => result?,
        };
        let evidence = if self.operation.metadata_approved() {
            let observation = subject
                .source
                .map(|_| self.operation.credential_observation());
            let metadata = self.operation.recording_metadata(request).await;
            if matches!(
                check_exchange_authority(catalog, actor, subject, &self.operation).await?,
                Traversal::Superseded
            ) {
                return Ok(Traversal::Superseded);
            }
            let metadata = match (observation.as_ref(), metadata) {
                (Some(observation), Ok(metadata)) => {
                    self.operation.credentials_verified(observation);
                    metadata
                }
                (Some(observation), Err(error)) => {
                    return Err(self.operation.with_credential_context(error, observation));
                }
                (None, result) => result?,
            };
            CandidateEvidence::Musical(Box::new(metadata))
        } else {
            CandidateEvidence::Legacy
        };
        Ok(Traversal::Current(Candidate {
            installation: self.installation.clone(),
            installation_revision: self.revision,
            operation: Arc::clone(&self.operation),
            details,
            sources,
            evidence,
        }))
    }
}

async fn check_exchange_authority(
    catalog: &Catalog,
    actor: &Actor,
    subject: &Subject<'_>,
    operation: &Operation,
) -> Result<Traversal<()>> {
    let mut tx = catalog.store.pool.begin().await?;
    let current = authority::current(&mut tx, actor, subject.source).await?;
    if matches!(current, Traversal::Current(())) {
        #[cfg(feature = "test-support")]
        tracing::debug!(
            target: "hito_core::authority_snapshot_test",
            point = "source_exchange",
            snapshot_current = true,
            transaction_open = <sqlx::sqlite::SqliteTransactionManager as sqlx::TransactionManager>::get_transaction_depth(&*tx) > 0,
            "Owned authority snapshot checkpoint"
        );
        operation.recheck(&mut tx).await?;
    }
    tx.commit().await?;
    if matches!(current, Traversal::Current(())) {
        operation.check_current().await?;
        let mut connection = catalog.store.pool.acquire().await?;
        return authority::current(&mut connection, actor, subject.source).await;
    }
    Ok(current)
}
