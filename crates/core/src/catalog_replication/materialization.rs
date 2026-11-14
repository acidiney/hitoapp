use super::{
    CatalogSnapshot, CatalogSnapshotMode, CatalogSnapshotPage, LocalRecordingFact,
    LocalReleaseAppearance, LocalSourceFact, SNAPSHOT_TTL, ServerRelayObservation, limit,
};
use crate::{Actor, HostDeviceId, Result, catalog::push_source_choice};
use sqlx::{QueryBuilder, Sqlite, SqliteConnection};
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

pub(super) async fn collect(
    connection: &mut SqliteConnection,
    actor: &Actor,
    host: &HostDeviceId,
    library: &str,
    id: Uuid,
) -> Result<Materialized> {
    #[cfg(test)]
    tracing::trace!(target: "hito_core::catalog_snapshot_stage_test", stage = "materialization_preflight");
    let (now, watermark): (i64, i64) = sqlx::query_as(
        "SELECT unixepoch(), COALESCE((SELECT max(sequence) FROM sync_events WHERE user_id=?),0)",
    )
    .bind(actor.id())
    .fetch_one(&mut *connection)
    .await?;
    let mut preflight = QueryBuilder::<Sqlite>::new("SELECT count(*),COALESCE(max(");
    preflight
        .push(RECORDING_BYTES)
        .push("),0) FROM recordings r WHERE r.library_id=")
        .push_bind(library.to_owned())
        .push(" AND ")
        .push(LOCAL_EVIDENCE);
    let (count, size): (i64, i64) = preflight
        .build_query_as()
        .fetch_one(&mut *connection)
        .await?;
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
        let mut query = recordings_query(library, cursor.as_ref());
        let rows: Vec<RecordingRow> = query.build_query_as().fetch_all(&mut *connection).await?;
        let Some(last) = rows.last() else {
            break;
        };
        cursor = Some((last.title.clone(), last.id.clone()));
        let source_facts = sources(connection, actor, host, &rows).await?;
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
                finish(&current, &mut pages, &mut bytes)?;
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
        }
    }
    finish(&current, &mut pages, &mut bytes)?;
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
) -> Result<Vec<Vec<LocalSourceFact>>> {
    let mut preflight = source_preflight(rows);
    let groups: Vec<(i64, i64, i64)> = preflight
        .build_query_as()
        .fetch_all(&mut *connection)
        .await?;
    if groups.len() != rows.len()
        || groups
            .iter()
            .any(|(_, count, size)| *count > MAX_SOURCES || *size > (MAX_FACT_BYTES / 6) as i64)
    {
        return Err(limit());
    }
    let mut query = source_query(rows, actor, host);
    let sources: Vec<SourceRow> = query.build_query_as().fetch_all(connection).await?;
    let mut facts: Vec<Vec<LocalSourceFact>> = (0..rows.len()).map(|_| Vec::new()).collect();
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
    }
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
