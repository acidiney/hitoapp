use std::time::Instant;

use sqlx::Row;
use uuid::Uuid;

use crate::{Error, admission::Administrator};

use super::{
    Corrections,
    types::{MAX_EXPECTED_REVISION, ReviewCorrection, ReviewReceipt},
};

impl Corrections {
    pub(crate) async fn review(
        &self,
        _: Administrator,
        correction_id: Uuid,
        request: ReviewCorrection,
    ) -> Result<(bool, ReviewReceipt), Error> {
        let started = Instant::now();
        if correction_id.get_version_num() != 4
            || request.request_id.get_version_num() != 4
            || !(1..=MAX_EXPECTED_REVISION).contains(&request.expected_revision)
        {
            return Err(Error::InvalidInput);
        }
        let mut transaction = self.storage.pool.begin_with("BEGIN IMMEDIATE").await?;
        let previous = sqlx::query(
            "SELECT correction_id, expected_revision, result_revision, decision FROM correction_review_receipts WHERE request_id = ?",
        )
        .bind(request.request_id.to_string())
        .fetch_optional(&mut *transaction)
        .await?;
        let (created, revision) = if let Some(previous) = previous {
            if previous.get::<&str, _>("correction_id") != correction_id.to_string()
                || previous.get::<i64, _>("expected_revision") != request.expected_revision
                || previous.get::<&str, _>("decision") != request.decision.as_str()
            {
                return Err(Error::SubmissionConflict);
            }
            (false, previous.get("result_revision"))
        } else {
            let current: Option<i64> = sqlx::query_scalar(
                "SELECT revision FROM correction_suggestions WHERE correction_id = ?",
            )
            .bind(correction_id.to_string())
            .fetch_optional(&mut *transaction)
            .await?;
            let current = current.ok_or(Error::NotFound)?;
            if current != request.expected_revision {
                return Err(Error::RevisionConflict);
            }
            let revision = request.expected_revision + 1;
            let changed = sqlx::query(
                "UPDATE correction_suggestions SET status = ?, revision = ? WHERE correction_id = ? AND revision = ?",
            )
            .bind(request.decision.status().as_str())
            .bind(revision)
            .bind(correction_id.to_string())
            .bind(request.expected_revision)
            .execute(&mut *transaction)
            .await?
            .rows_affected();
            if changed != 1 {
                return Err(Error::StorageUnavailable);
            }
            sqlx::query(
                "INSERT INTO correction_review_receipts (request_id, correction_id, expected_revision, result_revision, decision) VALUES (?, ?, ?, ?, ?)",
            )
            .bind(request.request_id.to_string())
            .bind(correction_id.to_string())
            .bind(request.expected_revision)
            .bind(revision)
            .bind(request.decision.as_str())
            .execute(&mut *transaction)
            .await?;
            (true, revision)
        };
        transaction.commit().await?;
        tracing::debug!(
            event = "community_correction_reviewed",
            outcome = if created { "created" } else { "replayed" },
            duration_ms = started.elapsed().as_millis() as u64
        );
        Ok((
            created,
            ReviewReceipt {
                request_id: request.request_id,
                correction_id,
                revision,
                decision: request.decision,
                status: request.decision.status(),
                validation_method: "maintainer",
            },
        ))
    }
}
