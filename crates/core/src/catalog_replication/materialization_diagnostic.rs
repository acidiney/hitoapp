use crate::Result;
use std::time::Instant;

#[cfg(test)]
#[derive(Clone, Copy)]
pub(in crate::catalog_replication) enum Phase {
    CatalogPreflight,
    RecordingPreflight,
    RecordingBatchQuery,
    SourceFactsQuery,
    SourceFactAssembly,
    RecordingFactAssembly,
    FinalPageEncoding,
}

#[cfg(test)]
impl Phase {
    fn code(self) -> usize {
        match self {
            Self::CatalogPreflight => 0,
            Self::RecordingPreflight => 1,
            Self::RecordingBatchQuery => 2,
            Self::SourceFactsQuery => 4,
            Self::SourceFactAssembly => 5,
            Self::RecordingFactAssembly => 6,
            Self::FinalPageEncoding => 7,
        }
    }
}

#[cfg(test)]
pub(in crate::catalog_replication) struct MaterializationDiagnostic {
    phase_us: [u64; 8],
    phase_calls: [u64; 8],
    active: Option<(Phase, Instant)>,
    outcome: u64,
    recordings: u64,
    batches: u64,
    source_rows: u64,
    source_facts: u64,
    pages: u64,
    bytes: u64,
}

#[cfg(test)]
impl MaterializationDiagnostic {
    pub(in crate::catalog_replication) fn new() -> Self {
        Self {
            phase_us: [0; 8],
            phase_calls: [0; 8],
            active: None,
            outcome: 0,
            recordings: 0,
            batches: 0,
            source_rows: 0,
            source_facts: 0,
            pages: 0,
            bytes: 0,
        }
    }

    pub(in crate::catalog_replication) fn start(&mut self, phase: Phase) {
        self.active = Some((phase, Instant::now()));
    }

    pub(in crate::catalog_replication) fn end(&mut self) {
        if let Some((phase, started)) = self.active.take() {
            let index = phase.code();
            self.phase_us[index] = self.phase_us[index].saturating_add(elapsed_us(started));
            self.phase_calls[index] = self.phase_calls[index].saturating_add(1);
        }
    }

    pub(in crate::catalog_replication) fn finish<T>(&mut self, result: &Result<T>) {
        self.outcome = if result.is_ok() { 1 } else { 2 };
    }

    pub(in crate::catalog_replication) fn recording_batch_finished(&mut self) {
        self.batches = self.batches.saturating_add(1);
    }

    pub(in crate::catalog_replication) fn recording_finished(&mut self) {
        self.recordings = self.recordings.saturating_add(1);
    }

    pub(in crate::catalog_replication) fn source_rows(&mut self, count: usize) {
        self.source_rows = self.source_rows.saturating_add(count as u64);
    }

    pub(in crate::catalog_replication) fn source_facts(&mut self, count: usize) {
        self.source_facts = self.source_facts.saturating_add(count as u64);
    }

    pub(in crate::catalog_replication) fn page_finished(&mut self, bytes: usize) {
        self.pages = self.pages.saturating_add(1);
        self.bytes = self.bytes.saturating_add(bytes as u64);
    }

    pub(in crate::catalog_replication) fn encoded_bytes(&self) -> u64 {
        self.bytes
    }
}

#[cfg(test)]
impl Drop for MaterializationDiagnostic {
    fn drop(&mut self) {
        let (active_phase, active_us) = self.active.map_or((0, 0), |(phase, started)| {
            (phase.code() as u64 + 1, elapsed_us(started))
        });
        tracing::trace!(
            target: "hito_core::catalog_snapshot_stage_test",
            stage = "materialization_summary",
            outcome_code = self.outcome,
            completed = self.outcome == 1,
            active_phase,
            active_partial_us = active_us,
            completed_batches = self.batches,
            completed_recordings = self.recordings,
            fetched_source_rows = self.source_rows,
            assembled_source_facts = self.source_facts,
            completed_pages = self.pages,
            encoded_bytes = self.bytes,
            catalog_preflight_us = self.phase_us[0],
            catalog_preflight_calls = self.phase_calls[0],
            recording_preflight_us = self.phase_us[1],
            recording_preflight_calls = self.phase_calls[1],
            recording_batch_query_us = self.phase_us[2],
            recording_batch_query_calls = self.phase_calls[2],
            source_preflight_query_us = self.phase_us[3],
            source_preflight_query_calls = self.phase_calls[3],
            source_facts_query_us = self.phase_us[4],
            source_facts_query_calls = self.phase_calls[4],
            source_fact_assembly_us = self.phase_us[5],
            source_fact_assembly_calls = self.phase_calls[5],
            recording_fact_assembly_us = self.phase_us[6],
            recording_fact_assembly_calls = self.phase_calls[6],
            final_page_encoding_us = self.phase_us[7],
            final_page_encoding_calls = self.phase_calls[7],
            "Catalog materialization diagnostic"
        );
    }
}

#[cfg(test)]
fn elapsed_us(started: Instant) -> u64 {
    started.elapsed().as_micros().min(u128::from(u64::MAX)) as u64
}
