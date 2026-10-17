use super::{lease::Lease, publication::Completion};
use crate::Result;
use hito_plugin_sdk::{PreparationReason, PreparationStatus, PreparationSubmission};
use sqlx::SqliteConnection;

pub(super) async fn retain(
    connection: &mut SqliteConnection,
    lease: &Lease,
    completion: &Completion,
) -> Result<()> {
    match completion {
        Completion::NotStarted(_) => {
            sqlx::query("UPDATE source_preparations SET
                    phase = CASE WHEN authority_cancelled = 1 THEN 'cancelled' ELSE 'queued' END,
                    submission_state = 'unsubmitted',dispatch_id = NULL,begin_attempts = max(begin_attempts - 1,0)
                    WHERE id = ? AND dispatch_id = ? AND submission_state IN ('submitting','unknown')")
                    .bind(&lease.id).bind(&lease.lease_token).execute(&mut *connection).await?;
        }
        Completion::Submitted(Ok(PreparationSubmission::Accepted { handle, status })) => {
            sqlx::query("UPDATE source_preparations SET submission_state = 'submitted',remote_handle = ?,remote_outcome = ?,
                    phase = CASE WHEN authority_cancelled = 1 THEN 'cancelled' ELSE 'preparing' END,
                    next_check_at = CASE WHEN authority_cancelled = 1 THEN NULL ELSE unixepoch() + 5 END,
                    updated_at = unixepoch() WHERE id = ? AND dispatch_id = ? AND submission_state IN ('submitting','unknown')")
                    .bind(handle).bind(terminal_outcome(status)).bind(&lease.id).bind(&lease.lease_token).execute(&mut *connection).await?;
        }
        Completion::Submitted(Ok(PreparationSubmission::NotSubmitted {
            reason,
            retry_after_seconds,
        })) => {
            let transient = matches!(
                reason,
                PreparationReason::RateLimited | PreparationReason::ProviderUnavailable
            );
            let retry = transient && lease.begin_attempts + 1 < 3;
            let phase = if retry { "queued" } else { "blocked" };
            let delay = retry.then(|| i64::from(retry_after_seconds.unwrap_or(5)));
            sqlx::query("UPDATE source_preparations SET
                    phase = CASE WHEN authority_cancelled = 1 THEN 'cancelled' ELSE ? END,
                    submission_state = 'unsubmitted',dispatch_id = NULL,reason = ?,
                    next_check_at = CASE WHEN authority_cancelled = 1 OR ? IS NULL THEN NULL ELSE unixepoch() + ? END,
                    updated_at = unixepoch() WHERE id = ? AND dispatch_id = ? AND submission_state IN ('submitting','unknown')")
                    .bind(phase).bind(super::publication::reason_code(*reason)).bind(delay).bind(delay)
                    .bind(&lease.id).bind(&lease.lease_token).execute(&mut *connection).await?;
        }
        Completion::Submitted(Err(_) | Ok(PreparationSubmission::SubmissionUnknown {})) => {
            sqlx::query("UPDATE source_preparations SET phase = 'submission_unknown',submission_state = 'unknown',
                    reason = 'provider_unavailable',next_check_at = NULL,updated_at = unixepoch()
                    WHERE id = ? AND dispatch_id = ? AND submission_state = 'submitting'")
                    .bind(&lease.id).bind(&lease.lease_token).execute(&mut *connection).await?;
        }
        Completion::Polled(Ok(Some(status))) => {
            if let Some(outcome) = terminal_outcome(status) {
                sqlx::query(
                    "UPDATE source_preparations SET remote_outcome = ?,updated_at = unixepoch()
                        WHERE id = ? AND submission_state = 'submitted' AND remote_outcome IS NULL",
                )
                .bind(outcome)
                .bind(&lease.id)
                .execute(&mut *connection)
                .await?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn terminal_outcome(status: &PreparationStatus) -> Option<&'static str> {
    match status {
        PreparationStatus::Ready {} => Some("ready"),
        PreparationStatus::Failed { .. } => Some("failed"),
        _ => None,
    }
}
