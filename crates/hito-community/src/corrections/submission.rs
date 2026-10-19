use std::time::Instant;

use sqlx::Row;
use uuid::Uuid;

use crate::{Error, admission::Reporter};

use super::{
    Corrections,
    types::{CorrectionReceipt, Relation, SuggestCorrection},
};

impl Corrections {
    pub(crate) async fn submit(
        &self,
        reporter: Reporter,
        mut suggestion: SuggestCorrection,
    ) -> Result<(bool, CorrectionReceipt), Error> {
        let started = Instant::now();
        suggestion.left.validate()?;
        suggestion.right.validate()?;
        if suggestion.submission_id.get_version_num() != 4 || suggestion.left == suggestion.right {
            return Err(Error::InvalidInput);
        }
        let Relation::SameRecording = suggestion.relation;
        if (
            suggestion.left.provider.as_str(),
            &suggestion.left.recording_id,
        ) > (
            suggestion.right.provider.as_str(),
            &suggestion.right.recording_id,
        ) {
            std::mem::swap(&mut suggestion.left, &mut suggestion.right);
        }
        let mut transaction = self.storage.pool.begin_with("BEGIN IMMEDIATE").await?;
        let reporter_id = reporter.0.to_string();
        let current: bool =
            sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM reporters WHERE id = ?)")
                .bind(&reporter_id)
                .fetch_one(&mut *transaction)
                .await?;
        if !current {
            return Err(Error::Unauthorized);
        }
        let previous = sqlx::query(
            "SELECT correction_id, left_provider, left_recording_id, right_provider, right_recording_id FROM correction_suggestions WHERE reporter_id = ? AND submission_id = ?",
        )
        .bind(&reporter_id)
        .bind(suggestion.submission_id.to_string())
        .fetch_optional(&mut *transaction)
        .await?;
        let (created, correction_id) = if let Some(previous) = previous {
            if previous.get::<String, _>("left_provider") != suggestion.left.provider.as_str()
                || previous.get::<String, _>("left_recording_id") != suggestion.left.recording_id
                || previous.get::<String, _>("right_provider") != suggestion.right.provider.as_str()
                || previous.get::<String, _>("right_recording_id") != suggestion.right.recording_id
            {
                return Err(Error::SubmissionConflict);
            }
            (
                false,
                Uuid::parse_str(&previous.get::<String, _>("correction_id"))
                    .map_err(|_| Error::StorageUnavailable)?,
            )
        } else {
            let recent: i64 = sqlx::query_scalar(
                "SELECT count(*) FROM (SELECT 1 FROM correction_suggestions INDEXED BY corrections_reporter_created WHERE reporter_id = ? AND created_at > unixepoch() - 60 LIMIT 10)",
            )
            .bind(&reporter_id)
            .fetch_one(&mut *transaction)
            .await?;
            let pending: i64 = sqlx::query_scalar(
                "SELECT count(*) FROM (SELECT 1 FROM correction_suggestions INDEXED BY corrections_reporter_status_sequence WHERE reporter_id = ? AND status = 'pending' LIMIT 32)",
            )
            .bind(&reporter_id)
            .fetch_one(&mut *transaction)
            .await?;
            let total_pending: i64 = sqlx::query_scalar(
                "SELECT count(*) FROM (SELECT 1 FROM correction_suggestions INDEXED BY corrections_status_sequence WHERE status = 'pending' LIMIT 1024)",
            )
            .fetch_one(&mut *transaction)
            .await?;
            if recent >= 10 || pending >= 32 || total_pending >= 1024 {
                return Err(Error::RateLimited);
            }
            let correction_id = Uuid::new_v4();
            sqlx::query(
                "INSERT INTO correction_suggestions (correction_id, reporter_id, submission_id, left_provider, left_recording_id, right_provider, right_recording_id, relation) VALUES (?, ?, ?, ?, ?, ?, ?, 'same_recording')",
            )
            .bind(correction_id.to_string())
            .bind(&reporter_id)
            .bind(suggestion.submission_id.to_string())
            .bind(suggestion.left.provider.as_str())
            .bind(&suggestion.left.recording_id)
            .bind(suggestion.right.provider.as_str())
            .bind(&suggestion.right.recording_id)
            .execute(&mut *transaction)
            .await?;
            (true, correction_id)
        };
        transaction.commit().await?;
        tracing::debug!(
            event = "community_correction_submitted",
            outcome = if created { "created" } else { "replayed" },
            duration_ms = started.elapsed().as_millis() as u64
        );
        Ok((
            created,
            CorrectionReceipt {
                correction_id,
                submission_id: suggestion.submission_id,
                status: "pending",
            },
        ))
    }
}
