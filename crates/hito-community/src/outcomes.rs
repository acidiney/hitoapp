use std::sync::Arc;

use sqlx::Row;
use uuid::Uuid;

use crate::{
    Error,
    admission::Reporter,
    storage::Storage,
    wire::{OutcomeKind, OutcomeReceipt, SubmitOutcome},
};

#[derive(Clone)]
pub(crate) struct Outcomes {
    storage: Arc<Storage>,
}

impl Outcomes {
    pub(crate) fn new(storage: Arc<Storage>) -> Self {
        Self { storage }
    }

    pub(crate) async fn submit(
        &self,
        reporter: Reporter,
        submission: SubmitOutcome,
    ) -> Result<(bool, OutcomeReceipt), Error> {
        submission.source.validate()?;
        if submission.submission_id.get_version_num() != 4 {
            return Err(Error::InvalidInput);
        }
        let OutcomeKind::SuccessfulPlayback = submission.outcome;
        let mut transaction = self.storage.pool.begin_with("BEGIN IMMEDIATE").await?;
        let current: bool =
            sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM reporters WHERE id = ?)")
                .bind(reporter.0.to_string())
                .fetch_one(&mut *transaction)
                .await?;
        if !current {
            return Err(Error::Unauthorized);
        }
        let previous = sqlx::query(
            "SELECT receipt_id, provider, recording_id FROM outcomes WHERE reporter_id = ? AND submission_id = ?",
        )
        .bind(reporter.0.to_string())
        .bind(submission.submission_id.to_string())
        .fetch_optional(&mut *transaction)
        .await?;
        let (created, receipt_id) = if let Some(previous) = previous {
            if previous.get::<String, _>("provider") != submission.source.provider.as_str()
                || previous.get::<String, _>("recording_id") != submission.source.recording_id
            {
                return Err(Error::SubmissionConflict);
            }
            let receipt = previous.get::<String, _>("receipt_id");
            (
                false,
                Uuid::parse_str(&receipt).map_err(|_| Error::StorageUnavailable)?,
            )
        } else {
            let receipt_id = Uuid::new_v4();
            sqlx::query(
                "INSERT INTO outcomes (reporter_id, submission_id, receipt_id, provider, recording_id) VALUES (?, ?, ?, ?, ?)",
            )
            .bind(reporter.0.to_string())
            .bind(submission.submission_id.to_string())
            .bind(receipt_id.to_string())
            .bind(submission.source.provider.as_str())
            .bind(&submission.source.recording_id)
            .execute(&mut *transaction)
            .await?;
            (true, receipt_id)
        };
        transaction.commit().await?;
        tracing::debug!(
            event = "community_outcome_accepted",
            outcome = if created { "created" } else { "replayed" }
        );
        Ok((
            created,
            OutcomeReceipt {
                receipt_id,
                submission_id: submission.submission_id,
                status: "accepted",
            },
        ))
    }
}
