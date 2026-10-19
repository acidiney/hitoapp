use std::time::Instant;

use sqlx::{Row, sqlite::SqliteRow};
use uuid::Uuid;

use crate::{
    Error,
    admission::Administrator,
    wire::{Provider, SourceRef},
};

use super::{
    Corrections,
    types::{CorrectionPage, CorrectionStatus, CorrectionSummary, ListCorrections, Relation},
};

impl Corrections {
    pub(crate) async fn list(
        &self,
        _: Administrator,
        query: ListCorrections,
    ) -> Result<CorrectionPage, Error> {
        let started = Instant::now();
        if query.after < 0 {
            return Err(Error::InvalidInput);
        }
        let rows = sqlx::query(
            "SELECT sequence, correction_id, left_provider, left_recording_id, right_provider, right_recording_id, status, revision, created_at FROM correction_suggestions INDEXED BY corrections_status_sequence WHERE status = ? AND sequence > ? ORDER BY sequence LIMIT 33",
        )
        .bind(query.status.as_str())
        .bind(query.after)
        .fetch_all(&self.storage.pool)
        .await?;
        let has_more = rows.len() > 32;
        let mut next_after = None;
        let mut suggestions = Vec::with_capacity(rows.len().min(32));
        for row in rows.iter().take(32) {
            next_after = Some(row.get("sequence"));
            let status = match row.get::<&str, _>("status") {
                "pending" => CorrectionStatus::Pending,
                "approved" => CorrectionStatus::Approved,
                "rejected" => CorrectionStatus::Rejected,
                _ => return Err(Error::StorageUnavailable),
            };
            suggestions.push(CorrectionSummary {
                correction_id: Uuid::parse_str(row.get("correction_id"))
                    .map_err(|_| Error::StorageUnavailable)?,
                left: source(row, "left_provider", "left_recording_id")?,
                right: source(row, "right_provider", "right_recording_id")?,
                relation: Relation::SameRecording,
                status,
                revision: row.get("revision"),
                created_at: row.get("created_at"),
            });
        }
        tracing::debug!(
            event = "community_corrections_listed",
            outcome = "listed",
            result_count = suggestions.len(),
            duration_ms = started.elapsed().as_millis() as u64
        );
        Ok(CorrectionPage {
            suggestions,
            next_after: if has_more { next_after } else { None },
        })
    }
}

fn source(row: &SqliteRow, provider_column: &str, id_column: &str) -> Result<SourceRef, Error> {
    let provider = match row.get::<&str, _>(provider_column) {
        "jamendo" => Provider::Jamendo,
        "youtube" => Provider::Youtube,
        _ => return Err(Error::StorageUnavailable),
    };
    Ok(SourceRef {
        provider,
        recording_id: row.get(id_column),
    })
}
