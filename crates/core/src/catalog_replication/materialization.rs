use super::{
    CatalogSnapshot, CatalogSnapshotMode, CatalogSnapshotPage, LocalRecordingFact,
    LocalReleaseAppearance, LocalSourceFact, SNAPSHOT_TTL, ServerRelayObservation, limit,
};
use crate::{Actor, HostDeviceId, Result, catalog::push_source_choice};
use sqlx::{QueryBuilder, Sqlite, SqliteConnection};
#[cfg(test)]
use std::time::Instant;
use uuid::Uuid;

pub(super) const MAX_RECORDINGS: usize = 100_000;
pub(super) const MAX_SOURCES: i64 = 128;
pub(super) const MAX_FACT_BYTES: usize = 64 * 1024;
pub(super) const MAX_PAGE_BYTES: usize = 256 * 1024;
pub(super) const MAX_EXPORT_BYTES: usize = 128 * 1024 * 1024;
const MAX_PAGE_FACTS: usize = 200;
const RECORDING_BATCH: usize = 64;
const LOCAL_EVIDENCE: &str = "EXISTS (SELECT 1 FROM sources s INDEXED BY sources_recording CROSS JOIN source_metadata m INDEXED BY sqlite_autoindex_source_metadata_1 ON m.source_id=s.id WHERE s.recording_id=r.id AND s.provider='local')";
const RECORDING_BYTES: &str = "length(CAST(r.id AS BLOB))+length(CAST(r.library_id AS BLOB))+length(CAST(r.title AS BLOB))+length(CAST(r.artist AS BLOB))";

#[derive(sqlx::FromRow)]
struct RecordingRow {
    id: String,
    library_id: String,
    title: String,
    artist: String,
    duration_ms: i64,
}

#[derive(sqlx::FromRow)]
struct SourceRow {
    ordinal: i64,
    id: String,
    observation_id: Option<String>,
    title: String,
    artist: String,
    duration_ms: i64,
    ready: bool,
    release_id: Option<String>,
    release_title: Option<String>,
    release_artist: Option<String>,
    disc_number: Option<i64>,
    track_number: Option<i64>,
}

pub(super) struct Materialized {
    pub metadata: CatalogSnapshot,
    pub pages: Vec<Vec<u8>>,
    pub bytes: usize,
}

#[cfg(test)]
#[derive(Clone, Copy)]
pub(super) enum Phase {
    CatalogPreflight,
    RecordingPreflight,
    RecordingBatchQuery,
    SourcePreflightQuery,
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
            Self::SourcePreflightQuery => 3,
            Self::SourceFactsQuery => 4,
            Self::SourceFactAssembly => 5,
            Self::RecordingFactAssembly => 6,
            Self::FinalPageEncoding => 7,
        }
    }
}

#[cfg(test)]
pub(super) struct MaterializationDiagnostic {
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
    pub(super) fn new() -> Self {
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

    pub(super) fn start(&mut self, phase: Phase) {
        self.active = Some((phase, Instant::now()));
    }

    pub(super) fn end(&mut self) {
        if let Some((phase, started)) = self.active.take() {
            let index = phase.code();
            self.phase_us[index] = self.phase_us[index].saturating_add(elapsed_us(started));
            self.phase_calls[index] = self.phase_calls[index].saturating_add(1);
        }
    }

    pub(super) fn finish<T>(&mut self, result: &Result<T>) {
        self.outcome = if result.is_ok() { 1 } else { 2 };
    }

    pub(super) fn recording_batch_finished(&mut self) {
        self.batches = self.batches.saturating_add(1);
    }

    pub(super) fn recording_finished(&mut self) {
        self.recordings = self.recordings.saturating_add(1);
    }

    pub(super) fn source_rows(&mut self, count: usize) {
        self.source_rows = self.source_rows.saturating_add(count as u64);
    }

    pub(super) fn source_facts(&mut self, count: usize) {
        self.source_facts = self.source_facts.saturating_add(count as u64);
    }

    pub(super) fn page_finished(&mut self, bytes: usize) {
        self.pages = self.pages.saturating_add(1);
        self.bytes = self.bytes.saturating_add(bytes as u64);
    }

    pub(super) fn encoded_bytes(&self) -> u64 {
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

pub(super) async fn collect(
    connection: &mut SqliteConnection,
    actor: &Actor,
    host: &HostDeviceId,
    library: &str,
    id: Uuid,
) -> Result<Materialized> {
    #[cfg(test)]
    {
        let mut diagnostic = MaterializationDiagnostic::new();
        let result = collect_inner(connection, actor, host, library, id, &mut diagnostic).await;
        diagnostic.finish(&result);
        result
    }
    #[cfg(not(test))]
    collect_inner(connection, actor, host, library, id).await
}

async fn collect_inner(
    connection: &mut SqliteConnection,
    actor: &Actor,
    host: &HostDeviceId,
    library: &str,
    id: Uuid,
    #[cfg(test)] diagnostic: &mut MaterializationDiagnostic,
) -> Result<Materialized> {
    #[cfg(test)]
    tracing::trace!(target: "hito_core::catalog_snapshot_stage_test", stage = "materialization_preflight");
    #[cfg(test)]
    diagnostic.start(Phase::CatalogPreflight);
    let preflight_result = sqlx::query_as(
        "SELECT unixepoch(), COALESCE((SELECT max(sequence) FROM sync_events WHERE user_id=?),0)",
    )
    .bind(actor.id())
    .fetch_one(&mut *connection)
    .await;
    #[cfg(test)]
    diagnostic.end();
    let (now, watermark): (i64, i64) = preflight_result?;
    #[cfg(test)]
    diagnostic.start(Phase::RecordingPreflight);
    let mut preflight = QueryBuilder::<Sqlite>::new("SELECT count(*),COALESCE(max(");
    preflight
        .push(RECORDING_BYTES)
        .push("),0) FROM recordings r WHERE r.library_id=")
        .push_bind(library.to_owned())
        .push(" AND ")
        .push(LOCAL_EVIDENCE);
    let recording_preflight = preflight.build_query_as().fetch_one(&mut *connection).await;
    #[cfg(test)]
    diagnostic.end();
    let (count, size): (i64, i64) = recording_preflight?;
    if count > MAX_RECORDINGS as i64 || size > (MAX_FACT_BYTES / 6) as i64 {
        return Err(limit());
    }
    #[cfg(test)]
    tracing::trace!(
        target: "hito_core::catalog_snapshot_stage_test",
        stage = "materialization_batches", catalog_recordings = count
    );
    let mut pages = Vec::new();
    let mut current = CatalogSnapshotPage {
        snapshot_id: id,
        ordinal: 0,
        recordings: Vec::new(),
        next_ordinal: None,
    };
    let mut bytes = 0;
    let mut current_bytes = 0;
    let mut cursor: Option<(String, String)> = None;
    loop {
        #[cfg(test)]
        diagnostic.start(Phase::RecordingBatchQuery);
        let mut query = recordings_query(library, cursor.as_ref());
        let recording_batch = query.build_query_as().fetch_all(&mut *connection).await;
        #[cfg(test)]
        diagnostic.end();
        let rows: Vec<RecordingRow> = recording_batch?;
        let Some(last) = rows.last() else {
            break;
        };
        cursor = Some((last.title.clone(), last.id.clone()));
        let source_facts = sources(
            connection,
            actor,
            host,
            &rows,
            #[cfg(test)]
            diagnostic,
        )
        .await?;
        #[cfg(test)]
        diagnostic.start(Phase::RecordingFactAssembly);
        let batch_result = (|| {
            for (row, sources) in rows.into_iter().zip(source_facts) {
                let fact = LocalRecordingFact {
                    id: row.id,
                    library_id: row.library_id,
                    title: row.title,
                    artist: row.artist,
                    duration_ms: row.duration_ms,
                    sources,
                };
                let encoded = serde_json::to_vec(&fact).map_err(|_| limit())?;
                if encoded.len() > MAX_FACT_BYTES {
                    return Err(limit());
                }
                if !current.recordings.is_empty()
                    && (current.recordings.len() == MAX_PAGE_FACTS
                        || current_bytes + encoded.len() + 512 > MAX_PAGE_BYTES)
                {
                    current.next_ordinal = Some(current.ordinal + 1);
                    let previous_bytes = bytes;
                    finish(&current, &mut pages, &mut bytes)?;
                    #[cfg(test)]
                    diagnostic.page_finished(bytes - previous_bytes);
                    current_bytes = 0;
                    current = CatalogSnapshotPage {
                        snapshot_id: id,
                        ordinal: current.ordinal + 1,
                        recordings: Vec::new(),
                        next_ordinal: None,
                    };
                }
                current_bytes += encoded.len() + 1;
                current.recordings.push(fact);
                #[cfg(test)]
                diagnostic.recording_finished();
            }
            Ok(())
        })();
        #[cfg(test)]
        diagnostic.end();
        batch_result?;
        #[cfg(test)]
        diagnostic.recording_batch_finished();
    }
    #[cfg(test)]
    diagnostic.start(Phase::FinalPageEncoding);
    let final_page = finish(&current, &mut pages, &mut bytes);
    #[cfg(test)]
    diagnostic.end();
    final_page?;
    #[cfg(test)]
    diagnostic.page_finished((bytes as u64).saturating_sub(diagnostic.encoded_bytes()) as usize);
    #[cfg(test)]
    tracing::trace!(
        target: "hito_core::catalog_snapshot_stage_test",
        stage = "materialization_encoded", catalog_recordings = count,
        encoded_pages = pages.len(), encoded_bytes = bytes
    );
    Ok(Materialized {
        metadata: CatalogSnapshot {
            id,
            account_id: actor.id().to_owned(),
            library_id: library.to_owned(),
            mode: CatalogSnapshotMode::SnapshotOnly,
            collection_watermark: watermark,
            recording_count: count as usize,
            page_count: pages.len() as u64,
            expires_at: now + SNAPSHOT_TTL.as_secs() as i64,
        },
        pages,
        bytes,
    })
}

fn finish(page: &CatalogSnapshotPage, pages: &mut Vec<Vec<u8>>, total: &mut usize) -> Result<()> {
    let encoded = serde_json::to_vec(page).map_err(|_| limit())?;
    if encoded.len() > MAX_PAGE_BYTES || encoded.len() > MAX_EXPORT_BYTES - *total {
        return Err(limit());
    }
    *total += encoded.len();
    pages.push(encoded);
    Ok(())
}

fn recordings_query(
    library: &str,
    cursor: Option<&(String, String)>,
) -> QueryBuilder<'static, Sqlite> {
    let mut query = QueryBuilder::new(
        "SELECT r.id,r.library_id,r.title,r.artist,r.duration_ms FROM recordings r WHERE r.library_id=",
    );
    query
        .push_bind(library.to_owned())
        .push(" AND ")
        .push(LOCAL_EVIDENCE);
    if let Some((title, id)) = cursor {
        query
            .push(" AND (r.title,r.id)>(")
            .push_bind(title.clone())
            .push(",")
            .push_bind(id.clone())
            .push(")");
    }
    query
        .push(" ORDER BY r.title,r.id LIMIT ")
        .push_bind(RECORDING_BATCH as i64);
    query
}

fn wanted(rows: &[RecordingRow]) -> QueryBuilder<'static, Sqlite> {
    let mut query = QueryBuilder::new("WITH wanted(ordinal,id) AS (VALUES ");
    let mut separated = query.separated(",");
    for (ordinal, row) in rows.iter().enumerate() {
        separated
            .push("(")
            .push_bind_unseparated(ordinal as i64)
            .push_unseparated(",")
            .push_bind_unseparated(row.id.clone())
            .push_unseparated(")");
    }
    query.push(") ");
    query
}

async fn sources(
    connection: &mut SqliteConnection,
    actor: &Actor,
    host: &HostDeviceId,
    rows: &[RecordingRow],
    #[cfg(test)] diagnostic: &mut MaterializationDiagnostic,
) -> Result<Vec<Vec<LocalSourceFact>>> {
    #[cfg(test)]
    diagnostic.start(Phase::SourcePreflightQuery);
    let mut preflight = source_preflight(rows);
    let preflight_result = preflight.build_query_as().fetch_all(&mut *connection).await;
    #[cfg(test)]
    diagnostic.end();
    let groups: Vec<(i64, i64, i64)> = preflight_result?;
    if groups.len() != rows.len()
        || groups
            .iter()
            .any(|(_, count, size)| *count > MAX_SOURCES || *size > (MAX_FACT_BYTES / 6) as i64)
    {
        return Err(limit());
    }
    #[cfg(test)]
    diagnostic.start(Phase::SourceFactsQuery);
    let mut query = source_query(rows, actor, host);
    let source_result = query.build_query_as().fetch_all(connection).await;
    #[cfg(test)]
    diagnostic.end();
    let sources: Vec<SourceRow> = source_result?;
    #[cfg(test)]
    diagnostic.source_rows(sources.len());
    let mut facts: Vec<Vec<LocalSourceFact>> = (0..rows.len()).map(|_| Vec::new()).collect();
    #[cfg(test)]
    diagnostic.start(Phase::SourceFactAssembly);
    let assembly = (|| {
        for row in sources {
            let ordinal = usize::try_from(row.ordinal).map_err(|_| limit())?;
            facts
                .get_mut(ordinal)
                .ok_or_else(limit)?
                .push(LocalSourceFact {
                    release: row.release_id.map(|id| LocalReleaseAppearance {
                        id,
                        entry_id: row.id.clone(),
                        title: row.release_title.unwrap_or_default(),
                        artist: row.release_artist,
                        disc_number: row.disc_number,
                        track_number: row.track_number,
                    }),
                    id: row.id,
                    observation_id: row.observation_id,
                    title: row.title,
                    artist: row.artist,
                    duration_ms: row.duration_ms,
                    server_relay: if row.ready {
                        ServerRelayObservation::Ready
                    } else {
                        ServerRelayObservation::Unavailable
                    },
                });
            #[cfg(test)]
            diagnostic.source_facts(1);
        }
        Ok(())
    })();
    #[cfg(test)]
    diagnostic.end();
    assembly?;
    Ok(facts)
}

fn source_preflight(rows: &[RecordingRow]) -> QueryBuilder<'static, Sqlite> {
    let mut preflight = wanted(rows);
    preflight.push("SELECT wanted.ordinal,count(*),COALESCE(sum(length(CAST(s.id AS BLOB))+COALESCE(length(CAST(m.observation_id AS BLOB)),0)+length(CAST(m.title AS BLOB))+length(CAST(m.artist AS BLOB))+COALESCE(length(CAST(a.id AS BLOB)),0)+COALESCE(length(CAST(a.title AS BLOB)),0)+COALESCE(length(CAST(a.artist AS BLOB)),0)),0)
        FROM wanted CROSS JOIN sources s INDEXED BY sources_recording ON s.recording_id=wanted.id AND s.provider='local'
        CROSS JOIN source_metadata m INDEXED BY sqlite_autoindex_source_metadata_1 ON m.source_id=s.id LEFT JOIN releases a INDEXED BY sqlite_autoindex_releases_1 ON a.id=m.release_id GROUP BY wanted.ordinal");
    preflight
}

fn source_query(
    rows: &[RecordingRow],
    actor: &Actor,
    host: &HostDeviceId,
) -> QueryBuilder<'static, Sqlite> {
    let mut query = wanted(rows);
    query.push("SELECT wanted.ordinal,s.id,m.observation_id,m.title,m.artist,m.duration_ms, CASE WHEN s.ready=1 AND (s.device_id IS NULL OR s.device_id=")
        .push_bind(host.as_str().to_owned()).push(") AND file.source_id IS NOT NULL AND folder.connected=1 AND folder.bound=1 AND ");
    push_source_choice(&mut query, actor.id());
    query.push(" THEN 1 ELSE 0 END AS ready,a.id AS release_id,a.title AS release_title,a.artist AS release_artist,m.disc_number,m.track_number
        FROM wanted CROSS JOIN sources s INDEXED BY sources_recording ON s.recording_id=wanted.id AND s.provider='local'
        CROSS JOIN source_metadata m INDEXED BY sqlite_autoindex_source_metadata_1 ON m.source_id=s.id
        LEFT JOIN releases a INDEXED BY sqlite_autoindex_releases_1 ON a.id=m.release_id
        LEFT JOIN local_files file INDEXED BY sqlite_autoindex_local_files_1 ON file.source_id=s.id
        LEFT JOIN music_folders folder INDEXED BY sqlite_autoindex_music_folders_1 ON folder.id=file.folder_id ORDER BY wanted.ordinal,s.id");
    query
}

#[cfg(test)]
pub(super) async fn explain_plans(
    connection: &mut SqliteConnection,
    actor: &Actor,
    host: &HostDeviceId,
    library: &str,
) {
    use sqlx::Execute;
    let mut initial = recordings_query(library, None);
    let rows: Vec<RecordingRow> = initial
        .build_query_as()
        .fetch_all(&mut *connection)
        .await
        .unwrap();
    let last = rows.last().unwrap();
    let cursor = (last.title.clone(), last.id.clone());
    for (name, mut query) in [
        ("recordings", recordings_query(library, Some(&cursor))),
        ("source_preflight", source_preflight(&rows)),
        ("source_facts", source_query(&rows, actor, host)),
    ] {
        let sql = format!("EXPLAIN QUERY PLAN {}", query.sql());
        let arguments = query.build().take_arguments().unwrap().unwrap();
        let rows: Vec<(i64, i64, i64, String)> = sqlx::query_as_with(&sql, arguments)
            .fetch_all(&mut *connection)
            .await
            .unwrap();
        eprintln!("actual_catalog_query={name} plan={rows:?}");
    }
}
