use super::{
    CatalogSnapshot, CatalogSnapshotMode, CatalogSnapshotPage, LocalRecordingFact,
    LocalReleaseAppearance, LocalSourceFact, SNAPSHOT_TTL, ServerRelayObservation, limit,
};
use crate::{Actor, HostDeviceId, Result, catalog::push_source_choice};
use sqlx::{QueryBuilder, Sqlite, SqliteConnection};
use uuid::Uuid;

#[cfg(test)]
#[path = "materialization_diagnostic.rs"]
pub(super) mod diagnostic;
#[cfg(test)]
pub(super) use diagnostic::{MaterializationDiagnostic, Phase};

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
struct SourceBatchRow {
    rejected: bool,
    ordinal: Option<i64>,
    id: Option<String>,
    observation_id: Option<String>,
    title: Option<String>,
    artist: Option<String>,
    duration_ms: Option<i64>,
    ready: Option<bool>,
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
    let mut current_raw = Vec::new();
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
        let batch_result: Result<()> = (|| {
            for (row, sources) in rows.into_iter().zip(source_facts) {
                let fact = LocalRecordingFact {
                    id: row.id,
                    library_id: row.library_id,
                    title: row.title,
                    artist: row.artist,
                    duration_ms: row.duration_ms,
                    sources,
                };
                let encoded = encode_fact(&fact)?;
                let encoded_len = encoded.get().len();
                if !current.recordings.is_empty()
                    && (current.recordings.len() == MAX_PAGE_FACTS
                        || current_bytes + encoded_len + 512 > MAX_PAGE_BYTES)
                {
                    current.next_ordinal = Some(current.ordinal + 1);
                    #[cfg(test)]
                    let previous_bytes = bytes;
                    finish(&current, &current_raw, &mut pages, &mut bytes)?;
                    #[cfg(test)]
                    diagnostic.page_finished(bytes - previous_bytes);
                    current_bytes = 0;
                    current_raw.clear();
                    current = CatalogSnapshotPage {
                        snapshot_id: id,
                        ordinal: current.ordinal + 1,
                        recordings: Vec::new(),
                        next_ordinal: None,
                    };
                }
                current_bytes += encoded_len + 1;
                current.recordings.push(fact);
                current_raw.push(encoded);
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
    let final_page = finish(&current, &current_raw, &mut pages, &mut bytes);
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

fn finish(
    page: &CatalogSnapshotPage,
    raw_recordings: &[Box<serde_json::value::RawValue>],
    pages: &mut Vec<Vec<u8>>,
    total: &mut usize,
) -> Result<()> {
    assert_eq!(page.recordings.len(), raw_recordings.len());
    let encoded = super::page_encoding::encode(page, raw_recordings).map_err(|_| limit())?;
    if encoded.len() > MAX_PAGE_BYTES || encoded.len() > MAX_EXPORT_BYTES - *total {
        return Err(limit());
    }
    *total += encoded.len();
    pages.push(encoded);
    Ok(())
}

fn encode_fact(fact: &LocalRecordingFact) -> Result<Box<serde_json::value::RawValue>> {
    let encoded = serde_json::value::to_raw_value(fact).map_err(|_| limit())?;
    if encoded.get().len() > MAX_FACT_BYTES {
        return Err(limit());
    }
    Ok(encoded)
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
    diagnostic.start(Phase::SourceFactsQuery);
    let mut query = source_batch_query(rows, actor, host);
    let source_result = query.build_query_as().fetch_all(&mut *connection).await;
    #[cfg(test)]
    diagnostic.end();
    let sources: Vec<SourceBatchRow> = source_result?;
    if sources.iter().any(|row| row.rejected) {
        return Err(limit());
    }
    #[cfg(test)]
    diagnostic.source_rows(sources.len());
    let mut facts: Vec<Vec<LocalSourceFact>> = (0..rows.len()).map(|_| Vec::new()).collect();
    #[cfg(test)]
    diagnostic.start(Phase::SourceFactAssembly);
    let assembly: Result<()> = (|| {
        for row in sources {
            let ordinal = usize::try_from(row.ordinal.ok_or_else(limit)?).map_err(|_| limit())?;
            let id = row.id.ok_or_else(limit)?;
            facts
                .get_mut(ordinal)
                .ok_or_else(limit)?
                .push(LocalSourceFact {
                    release: row.release_id.map(|release_id| LocalReleaseAppearance {
                        id: release_id,
                        entry_id: id.clone(),
                        title: row.release_title.unwrap_or_default(),
                        artist: row.release_artist,
                        disc_number: row.disc_number,
                        track_number: row.track_number,
                    }),
                    id,
                    observation_id: row.observation_id,
                    title: row.title.ok_or_else(limit)?,
                    artist: row.artist.ok_or_else(limit)?,
                    duration_ms: row.duration_ms.ok_or_else(limit)?,
                    server_relay: if row.ready.ok_or_else(limit)? {
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

fn source_batch_query(
    rows: &[RecordingRow],
    actor: &Actor,
    host: &HostDeviceId,
) -> QueryBuilder<'static, Sqlite> {
    let mut query = wanted(rows);
    query.push(", stats AS MATERIALIZED (
        SELECT wanted.ordinal AS ordinal,count(*) AS source_count,
            COALESCE(sum(length(CAST(s.id AS BLOB))
                +COALESCE(length(CAST(m.observation_id AS BLOB)),0)
                +length(CAST(m.title AS BLOB))
                +length(CAST(m.artist AS BLOB))
                +COALESCE(length(CAST(a.id AS BLOB)),0)
                +COALESCE(length(CAST(a.title AS BLOB)),0)
                +COALESCE(length(CAST(a.artist AS BLOB)),0)),0) AS source_bytes
        FROM wanted CROSS JOIN sources s INDEXED BY sources_recording
            ON s.recording_id=wanted.id AND s.provider='local'
        CROSS JOIN source_metadata m INDEXED BY sqlite_autoindex_source_metadata_1
            ON m.source_id=s.id
        LEFT JOIN releases a INDEXED BY sqlite_autoindex_releases_1 ON a.id=m.release_id
        GROUP BY wanted.ordinal
    ), admission AS MATERIALIZED (
        SELECT CASE WHEN (SELECT count(*) FROM stats)=(SELECT count(*) FROM wanted)
            AND NOT EXISTS (SELECT 1 FROM stats WHERE source_count>");
    query
        .push_bind(MAX_SOURCES)
        .push(" OR source_bytes>")
        .push_bind((MAX_FACT_BYTES / 6) as i64)
        .push(") THEN 1 ELSE 0 END AS allowed
    ), facts AS (
        SELECT 0 AS rejected,wanted.ordinal,
            CASE WHEN admission.allowed=1 THEN s.id ELSE NULL END AS id,
            CASE WHEN admission.allowed=1 THEN m.observation_id ELSE NULL END AS observation_id,
            CASE WHEN admission.allowed=1 THEN m.title ELSE NULL END AS title,
            CASE WHEN admission.allowed=1 THEN m.artist ELSE NULL END AS artist,
            CASE WHEN admission.allowed=1 THEN m.duration_ms ELSE NULL END AS duration_ms,
            CASE WHEN admission.allowed=1 THEN CASE WHEN s.ready=1 AND (s.device_id IS NULL OR s.device_id=");
    query.push_bind(host.as_str().to_owned());
    query.push(") AND file.source_id IS NOT NULL AND folder.connected=1 AND folder.bound=1 AND ");
    push_source_choice(&mut query, actor.id());
    query.push(" THEN 1 ELSE 0 END ELSE NULL END AS ready,
            CASE WHEN admission.allowed=1 THEN a.id ELSE NULL END AS release_id,
            CASE WHEN admission.allowed=1 THEN a.title ELSE NULL END AS release_title,
            CASE WHEN admission.allowed=1 THEN a.artist ELSE NULL END AS release_artist,
            CASE WHEN admission.allowed=1 THEN m.disc_number ELSE NULL END AS disc_number,
            CASE WHEN admission.allowed=1 THEN m.track_number ELSE NULL END AS track_number
        FROM admission CROSS JOIN wanted
        CROSS JOIN sources s INDEXED BY sources_recording ON s.recording_id=wanted.id AND s.provider='local'
        CROSS JOIN source_metadata m INDEXED BY sqlite_autoindex_source_metadata_1 ON m.source_id=s.id
        LEFT JOIN releases a INDEXED BY sqlite_autoindex_releases_1 ON a.id=m.release_id
        LEFT JOIN local_files file INDEXED BY sqlite_autoindex_local_files_1 ON file.source_id=s.id
        LEFT JOIN music_folders folder INDEXED BY sqlite_autoindex_music_folders_1 ON folder.id=file.folder_id
        WHERE admission.allowed=1
        UNION ALL
        SELECT 1,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL
        FROM admission WHERE allowed=0
    )
    SELECT rejected,ordinal,id,observation_id,title,artist,duration_ms,ready,
        release_id,release_title,release_artist,disc_number,track_number
    FROM facts ORDER BY 1,2,3");
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
        ("source_batch", source_batch_query(&rows, actor, host)),
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

#[cfg(test)]
mod encoding_limits_tests {
    use super::*;

    fn fact(title: String) -> LocalRecordingFact {
        LocalRecordingFact {
            id: "recording".into(),
            library_id: "library".into(),
            title,
            artist: "artist".into(),
            duration_ms: 0,
            sources: Vec::new(),
        }
    }

    #[test]
    fn cached_fact_encoding_rejects_the_fact_limit() {
        let oversized = fact("x".repeat(MAX_FACT_BYTES));
        assert!(matches!(
            encode_fact(&oversized),
            Err(crate::Error::Conflict(_))
        ));
    }

    #[test]
    fn cached_page_encoding_rejects_page_and_export_limits() {
        let oversized_fact = fact("x".repeat(MAX_PAGE_BYTES));
        let page = CatalogSnapshotPage {
            snapshot_id: Uuid::nil(),
            ordinal: 0,
            recordings: vec![oversized_fact],
            next_ordinal: None,
        };
        let raw = page
            .recordings
            .iter()
            .map(serde_json::value::to_raw_value)
            .collect::<serde_json::Result<Vec<_>>>()
            .unwrap();
        assert!(matches!(
            finish(&page, &raw, &mut Vec::new(), &mut 0),
            Err(crate::Error::Conflict(_))
        ));

        let empty = CatalogSnapshotPage {
            snapshot_id: Uuid::nil(),
            ordinal: 0,
            recordings: Vec::new(),
            next_ordinal: None,
        };
        assert!(matches!(
            finish(&empty, &[], &mut Vec::new(), &mut (MAX_EXPORT_BYTES - 1)),
            Err(crate::Error::Conflict(_))
        ));
    }
}
