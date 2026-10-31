use super::LibraryStorage;
use crate::{Actor, Error, Result, artist_catalog::erasure, identity::library_access};
use serde::Serialize;
use sqlx::SqliteConnection;
use std::time::Instant;
use utoipa::ToSchema;
use uuid::Uuid;

/// How long every source of a library recording must have been unavailable before maintenance
/// removes the recording.
pub(crate) const RETENTION_SECONDS: i64 = 30 * 24 * 60 * 60;
const BATCH: usize = 50;
const MAINTENANCE_LIMIT: i64 = 500;
const ADMINISTRATOR_LIMIT: i64 = 2000;
const REPORT_LIMIT: i64 = 100_000;

/// Tables whose rows mean an account still uses the recording. Any row, including an unlike or a
/// removed playlist entry, keeps the recording.
pub(crate) const REFERENCING_TABLES: [&str; 25] = [
    "likes",
    "listening_progress",
    "playlist_entries",
    "download_files",
    "download_intents",
    "download_playlist_members",
    "download_playlist_retry_waiting",
    "download_retentions",
    "audiobook_bookmarks",
    "audiobook_continuations",
    "personal_recording_metadata",
    "podcast_episodes",
    "provider_items",
    "recording_source_choices",
    "source_choice_receipts",
    "source_choice_reset_receipts",
    "source_resolutions",
    "source_resolution_receipts",
    "source_preparations",
    "artwork_fetches",
    "recording_artwork",
    "recording_artist_identity_choices",
    "album_edition_choices",
    "recording_artist_association_changes",
    "artist_identity_change_items",
];

/// Tables whose rows belong to the recording itself and leave with it.
#[cfg(test)]
pub(crate) const REMOVED_TABLES: [&str; 19] = [
    "account_recording_credits",
    "account_source_credits",
    "artist_match_checks",
    "artist_name_checks",
    "artist_provider_checks",
    "local_files",
    "recording_artist_association_change_order",
    "metadata_observations",
    "recording_artist_associations",
    "recording_artist_url_observations",
    "recording_evidence",
    "recording_isrc_observations",
    "recording_musicbrainz_observations",
    "shared_artist_correction_subjects",
    "source_artist_credits",
    "source_genres",
    "source_metadata",
    "source_unavailability",
    "sources",
];

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, ToSchema)]
pub struct MissingRecordingsRemoval {
    /// Recordings removed by this call.
    pub removed: i64,
    /// Recordings whose files are gone but that an account still uses, which stay.
    pub kept_referenced: i64,
    /// More removable recordings remain; run the action again.
    pub more: bool,
}

#[derive(Clone, Copy)]
enum Trigger {
    Maintenance,
    Administrator,
}

impl Trigger {
    fn name(self) -> &'static str {
        match self {
            Self::Maintenance => "maintenance",
            Self::Administrator => "administrator",
        }
    }
}

fn gone() -> &'static str {
    "r.library_id IN (SELECT id FROM libraries WHERE owner_id IS NULL)
     AND (?2 IS NULL OR r.library_id = ?2) AND r.id > ?3 AND (?6 IS NULL OR r.id = ?6)
     AND r.id IN (SELECT s.recording_id FROM source_unavailability u JOIN sources s ON s.id = u.source_id WHERE u.since <= ?1)
     AND NOT EXISTS (SELECT 1 FROM sources s LEFT JOIN source_unavailability u ON u.source_id = s.id
         WHERE s.recording_id = r.id AND (s.ready = 1 OR s.provider <> 'local' OR u.since IS NULL OR u.since > ?1
             OR EXISTS (SELECT 1 FROM local_files f JOIN music_folders m ON m.id = f.folder_id
                 WHERE f.source_id = s.id AND NOT (m.connected = 1 AND m.bound = 1 AND m.host_device = ?5
                     AND (SELECT j.state FROM jobs j WHERE j.folder_id = m.id ORDER BY j.rowid DESC LIMIT 1) = 'complete'
                     AND m.last_scan_at >= u.since))))"
}

fn unreferenced() -> String {
    let mut clauses: Vec<String> = REFERENCING_TABLES
        .iter()
        .map(|table| format!("NOT EXISTS (SELECT 1 FROM {table} WHERE recording_id = r.id)"))
        .collect();
    clauses.push(
        "NOT EXISTS (SELECT 1 FROM recording_artist_associations WHERE recording_id = r.id AND evidence = 'user_confirmed')"
            .into(),
    );
    clauses.push(
        "NOT EXISTS (SELECT 1 FROM community_reports c JOIN sources s ON s.id = c.source_id WHERE s.recording_id = r.id)"
            .into(),
    );
    clauses.join(" AND ")
}

impl LibraryStorage {
    /// Removes the library recordings whose files are all gone, immediately and whatever their
    /// age. Recordings an account still uses are kept and counted.
    #[tracing::instrument(level = "info", skip_all, err(Debug), fields(operation = "missing_recordings_remove", request_id = %Uuid::new_v4(), account_id = actor.id(), library_id = library))]
    pub async fn remove_missing_recordings(
        &self,
        actor: &Actor,
        library: &str,
    ) -> Result<MissingRecordingsRemoval> {
        actor.require_admin()?;
        let mut connection = self.store.pool.acquire().await?;
        library_access(&mut connection, actor, library).await?;
        crate::identity::shared_library(&mut connection, library).await?;
        drop(connection);
        let unsettled: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM music_folders m WHERE m.library_id = ?1 AND m.host_device = ?2 AND m.connected = 1
               AND (m.bound = 0 OR COALESCE((SELECT j.state FROM jobs j WHERE j.folder_id = m.id ORDER BY j.rowid DESC LIMIT 1), '') <> 'complete'))",
        )
        .bind(library)
        .bind(self.host.as_str())
        .fetch_one(&self.store.pool)
        .await?;
        if unsettled
            || self
                .folders(actor, library)
                .await?
                .iter()
                .any(|folder| !folder.available)
        {
            return Err(Error::Conflict(
                "Connect every music folder of this library, check that its drive is available and let its scan finish before removing missing tracks".into(),
            ));
        }
        self.purge(
            Trigger::Administrator,
            Some(library),
            0,
            ADMINISTRATOR_LIMIT,
        )
        .await
    }

    /// One bounded maintenance pass: removes library recordings whose every file has been gone
    /// longer than the retention window and that no account uses.
    #[tracing::instrument(level = "info", skip_all, err(Debug), fields(operation = "missing_recordings_purge", request_id = %Uuid::new_v4()))]
    pub async fn purge_expired_missing_recordings(&self) -> Result<MissingRecordingsRemoval> {
        self.purge(
            Trigger::Maintenance,
            None,
            RETENTION_SECONDS,
            MAINTENANCE_LIMIT,
        )
        .await
    }

    async fn purge(
        &self,
        trigger: Trigger,
        library: Option<&str>,
        window: i64,
        limit: i64,
    ) -> Result<MissingRecordingsRemoval> {
        let started = Instant::now();
        let cutoff: i64 = sqlx::query_scalar("SELECT unixepoch()")
            .fetch_one(&self.store.pool)
            .await?;
        let cutoff = cutoff - window;
        let selection = self.selection(library, cutoff);
        let mut removed = 0;
        let mut failed = 0;
        let mut cursor = String::new();
        while removed + failed < limit {
            let take = BATCH.min((limit - removed - failed) as usize) as i64;
            let mut tx = self.store.pool.begin_with("BEGIN IMMEDIATE").await?;
            let batch = selection.candidates(&mut tx, &cursor, None, take).await?;
            let Some(last) = batch.last().cloned() else {
                tx.rollback().await?;
                break;
            };
            let outcome = remove_all(&mut tx, &batch).await;
            if outcome.is_ok() {
                tx.commit().await?;
                removed += batch.len() as i64;
            } else {
                tx.rollback().await?;
                for recording in &batch {
                    match self.purge_one(&selection, recording).await {
                        Outcome::Removed => removed += 1,
                        Outcome::Skipped => {}
                        Outcome::Failed => failed += 1,
                    }
                }
            }
            cursor = last;
        }
        let more = removed + failed >= limit && self.any_removable(&selection, &cursor).await?;
        let kept_referenced: i64 = sqlx::query_scalar(&format!(
            "SELECT count(*) FROM (SELECT 1 FROM recordings r WHERE {} AND NOT ({}) LIMIT {REPORT_LIMIT})",
            gone(),
            unreferenced()
        ))
        .bind(cutoff)
        .bind(library)
        .bind("")
        .bind(0_i64)
        .bind(self.host.as_str())
        .bind(None::<String>)
        .fetch_one(&self.store.pool)
        .await?;
        tracing::info!(
            event = "missing_recordings_purged",
            trigger = trigger.name(),
            removed,
            failed,
            kept_referenced,
            more,
            elapsed_ms = started.elapsed().as_millis() as u64,
            "Missing recordings removed"
        );
        if failed > 0 {
            return Err(Error::Internal(
                "Some missing recordings could not be removed".into(),
            ));
        }
        Ok(MissingRecordingsRemoval {
            removed,
            kept_referenced,
            more,
        })
    }

    fn selection(&self, library: Option<&str>, cutoff: i64) -> Selection {
        Selection {
            select: format!(
                "SELECT r.id FROM recordings r WHERE {} AND {} ORDER BY r.id LIMIT ?4",
                gone(),
                unreferenced()
            ),
            cutoff,
            library: library.map(str::to_owned),
            host: self.host.as_str().to_owned(),
        }
    }

    async fn purge_one(&self, selection: &Selection, recording: &str) -> Outcome {
        let outcome = async {
            let mut tx = self.store.pool.begin_with("BEGIN IMMEDIATE").await?;
            if selection
                .candidates(&mut tx, "", Some(recording), 1)
                .await?
                .is_empty()
            {
                return Ok::<_, Error>(Outcome::Skipped);
            }
            remove_all(&mut tx, std::slice::from_ref(&recording.to_owned())).await?;
            tx.commit().await?;
            Ok(Outcome::Removed)
        }
        .await;
        outcome.unwrap_or_else(|_| {
            tracing::warn!(
                event = "missing_recording_removal_failed",
                "A missing recording could not be removed and stays for the next pass"
            );
            Outcome::Failed
        })
    }

    async fn any_removable(&self, selection: &Selection, cursor: &str) -> Result<bool> {
        let mut connection = self.store.pool.acquire().await?;
        Ok(!selection
            .candidates(&mut connection, cursor, None, 1)
            .await?
            .is_empty())
    }
}

enum Outcome {
    Removed,
    Skipped,
    Failed,
}

struct Selection {
    select: String,
    cutoff: i64,
    library: Option<String>,
    host: String,
}

impl Selection {
    async fn candidates(
        &self,
        connection: &mut SqliteConnection,
        after: &str,
        only: Option<&str>,
        limit: i64,
    ) -> Result<Vec<String>> {
        Ok(sqlx::query_scalar(&self.select)
            .bind(self.cutoff)
            .bind(&self.library)
            .bind(after)
            .bind(limit)
            .bind(&self.host)
            .bind(only)
            .fetch_all(connection)
            .await?)
    }
}

async fn remove_all(connection: &mut SqliteConnection, recordings: &[String]) -> Result<()> {
    for recording in recordings {
        erasure::erase_recording_corrections(connection, recording).await?;
        for statement in REMOVAL {
            sqlx::query(statement)
                .bind(recording)
                .execute(&mut *connection)
                .await?;
        }
    }
    Ok(())
}

const REMOVAL: [&str; 10] = [
    "UPDATE recordings SET metadata_source_id = NULL, metadata_observation_id = NULL WHERE id = ?1",
    "DELETE FROM account_source_credits WHERE recording_id = ?1",
    "DELETE FROM account_recording_credits WHERE recording_id = ?1",
    "DELETE FROM source_artist_credits WHERE source_id IN (SELECT id FROM sources WHERE recording_id = ?1)",
    "DELETE FROM source_metadata WHERE source_id IN (SELECT id FROM sources WHERE recording_id = ?1)",
    "DELETE FROM metadata_observations WHERE recording_id = ?1",
    "DELETE FROM recording_evidence WHERE recording_id = ?1",
    "DELETE FROM local_files WHERE source_id IN (SELECT id FROM sources WHERE recording_id = ?1)",
    "DELETE FROM sources WHERE recording_id = ?1",
    "DELETE FROM recordings WHERE id = ?1",
];

#[cfg(test)]
mod tests {
    use super::{Outcome, REFERENCING_TABLES, REMOVED_TABLES};
    use crate::store::Store;
    use crate::test_support::{support, work_meter::WorkMeter};

    const FIXED_WORK: u64 = 100_000;

    async fn seed(pool: &sqlx::SqlitePool, library: &str, prefix: &str, count: usize, ready: bool) {
        let mut tx = pool.begin().await.unwrap();
        for index in 0..count {
            let id = format!("{prefix}-{index:05}");
            sqlx::query(
                "INSERT INTO recordings(id, library_id, title, artist) VALUES (?1, ?2, ?1, 'Seed')",
            )
            .bind(&id)
            .bind(library)
            .execute(&mut *tx)
            .await
            .unwrap();
            sqlx::query("INSERT INTO sources(id, recording_id, provider, provider_id, device_id, ready) VALUES (?1, ?1, 'local', ?1, 'work', ?2)")
                .bind(&id)
                .bind(ready)
                .execute(&mut *tx)
                .await
                .unwrap();
        }
        sqlx::query("UPDATE source_unavailability SET since = unixepoch() - 40 * 86400")
            .execute(&mut *tx)
            .await
            .unwrap();
        tx.commit().await.unwrap();
    }

    #[tokio::test]
    async fn a_retry_after_a_rolled_back_batch_rechecks_eligibility() {
        let (_directory, hito, _actor, library, _music) = support::library("recheck").await;
        let pool = hito.storage.store.pool.clone();
        seed(&pool, &library, "late", 1, false).await;
        sqlx::query("UPDATE sources SET ready = 1 WHERE id = 'late-00000'")
            .execute(&pool)
            .await
            .unwrap();
        let selection = hito.storage.selection(None, i64::MAX);
        assert!(matches!(
            hito.storage.purge_one(&selection, "late-00000").await,
            Outcome::Skipped
        ));
        let left: i64 =
            sqlx::query_scalar("SELECT count(*) FROM recordings WHERE id = 'late-00000'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(left, 1, "a source that came back keeps its recording");
    }

    #[tokio::test]
    async fn personal_decisions_keep_a_recording() {
        for table in [
            "personal_recording_metadata",
            "recording_artist_identity_choices",
            "album_edition_choices",
            "recording_artist_association_changes",
            "artist_identity_change_items",
        ] {
            assert!(REFERENCING_TABLES.contains(&table), "{table}");
        }
    }

    #[tokio::test]
    async fn a_pass_does_not_scale_with_the_recordings_that_are_still_available() {
        let (_directory, hito, _actor, library, _music) = support::library("work").await;
        let pool = hito.storage.store.pool.clone();
        let meter = WorkMeter::install(&pool).await;
        let mut measured = Vec::new();
        let mut present = 0;
        for total in [200, 6000] {
            seed(
                &pool,
                &library,
                &format!("present{total}"),
                total - present,
                true,
            )
            .await;
            present = total;
            seed(&pool, &library, &format!("gone{total}"), 50, false).await;
            let (outcome, work) = meter
                .measure(async {
                    hito.storage
                        .purge_expired_missing_recordings()
                        .await
                        .unwrap()
                })
                .await;
            assert_eq!(outcome.removed, 50);
            measured.push(work);
        }
        meter.remove(&pool).await;
        eprintln!(
            "Missing recordings VM_steps small={} large={}",
            measured[0], measured[1]
        );
        let budget = measured[0] + FIXED_WORK;
        assert!(
            measured[1] < budget,
            "A pass used {} VM steps beside 6000 available recordings after {} beside 200; budget {budget}",
            measured[1],
            measured[0]
        );
    }

    #[tokio::test]
    async fn every_table_that_can_name_a_recording_or_source_is_classified() {
        let directory = tempfile::tempdir().unwrap();
        let store = Store::open(&directory.path().join("classified.db"))
            .await
            .unwrap();
        let naming: Vec<String> = sqlx::query_scalar(
            "SELECT DISTINCT m.name FROM sqlite_master m, pragma_foreign_key_list(m.name) f
             WHERE m.type = 'table' AND f.\"table\" IN ('recordings', 'sources', 'source_metadata')",
        )
        .fetch_all(&store.pool)
        .await
        .unwrap();
        let by_column: Vec<String> = sqlx::query_scalar(
            "SELECT DISTINCT m.name FROM sqlite_master m, pragma_table_info(m.name) c
             WHERE m.type = 'table' AND m.name NOT LIKE 'sqlite_%'
               AND c.name IN ('recording_id', 'source_id', 'public_recording_id')",
        )
        .fetch_all(&store.pool)
        .await
        .unwrap();
        let naming: Vec<String> = naming.into_iter().chain(by_column).collect();
        let unclassified: Vec<&String> = naming
            .iter()
            .filter(|table| {
                !REFERENCING_TABLES.contains(&table.as_str())
                    && !REMOVED_TABLES.contains(&table.as_str())
                    && ![
                        "recordings",
                        "community_reports",
                        "podcast_sources",
                        "provider_sources",
                    ]
                    .contains(&table.as_str())
            })
            .collect();
        assert!(
            unclassified.is_empty(),
            "classify these tables as referencing or removed with the recording: {unclassified:?}"
        );
    }
}
