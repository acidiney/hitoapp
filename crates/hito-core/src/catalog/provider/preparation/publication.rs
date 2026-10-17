use super::{Catalog, authority, lease::Lease};
use crate::{Error, Result};
use hito_plugin_sdk::{PreparationReason, PreparationStatus, PreparationSubmission};
use sqlx::SqliteConnection;

pub(super) enum Completion {
    NotStarted(Option<Error>),
    Submitted(Result<PreparationSubmission>),
    Polled(Result<Option<PreparationStatus>>),
}

impl Catalog {
    pub(super) async fn finish_source_preparation(
        &self,
        lease: &Lease,
        completion: Completion,
        fenced: bool,
    ) -> Result<&'static str> {
        let mut tx = self.store.pool.begin_with("BEGIN IMMEDIATE").await?;
        match &completion {
            Completion::NotStarted(_) => {
                sqlx::query("UPDATE source_preparations SET
                    phase = CASE WHEN authority_cancelled = 1 THEN 'cancelled' ELSE 'queued' END,
                    submission_state = 'unsubmitted',dispatch_id = NULL,begin_attempts = max(begin_attempts - 1,0)
                    WHERE id = ? AND dispatch_id = ? AND submission_state IN ('submitting','unknown')")
                    .bind(&lease.id).bind(&lease.lease_token).execute(&mut *tx).await?;
            }
            Completion::Submitted(Ok(PreparationSubmission::Accepted { handle, .. })) => {
                sqlx::query("UPDATE source_preparations SET submission_state = 'submitted',remote_handle = ?,
                    phase = CASE WHEN authority_cancelled = 1 THEN 'cancelled' ELSE 'preparing' END,
                    next_check_at = CASE WHEN authority_cancelled = 1 THEN NULL ELSE unixepoch() + 5 END,
                    updated_at = unixepoch() WHERE id = ? AND dispatch_id = ? AND submission_state IN ('submitting','unknown')")
                    .bind(handle).bind(&lease.id).bind(&lease.lease_token).execute(&mut *tx).await?;
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
                    .bind(phase).bind(reason_code(*reason)).bind(delay).bind(delay)
                    .bind(&lease.id).bind(&lease.lease_token).execute(&mut *tx).await?;
            }
            Completion::Submitted(Err(_) | Ok(PreparationSubmission::SubmissionUnknown {})) => {
                sqlx::query("UPDATE source_preparations SET phase = 'submission_unknown',submission_state = 'unknown',
                    reason = 'provider_unavailable',next_check_at = NULL,updated_at = unixepoch()
                    WHERE id = ? AND dispatch_id = ? AND submission_state = 'submitting'")
                    .bind(&lease.id).bind(&lease.lease_token).execute(&mut *tx).await?;
            }
            _ => {}
        }
        let owned: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM source_preparations
            WHERE id = ? AND lease_token = ? AND lease_until > unixepoch())",
        )
        .bind(&lease.id)
        .bind(&lease.lease_token)
        .fetch_one(&mut *tx)
        .await?;
        if !owned {
            tx.commit().await?;
            return Ok("receipt_retained");
        }
        authority::cancel_stale_on(&mut tx, &lease.id).await?;
        let authorized = match authority::check_on(&mut tx, &lease.id).await {
            Ok(()) => true,
            Err(Error::Conflict(_)) => false,
            Err(error) => return Err(error),
        };
        let outcome = if !authorized {
            "authority_changed"
        } else {
            match completion {
                Completion::NotStarted(error) => {
                    let (phase, reason, delay) = match error {
                        Some(Error::Invalid(_)) => {
                            ("blocked", Some("authorization_required"), None)
                        }
                        Some(Error::Internal(_)) => ("blocked", Some("invalid_response"), None),
                        _ => (
                            if lease.submission_state == "submitted" {
                                "preparing"
                            } else {
                                "queued"
                            },
                            None,
                            Some(1),
                        ),
                    };
                    schedule(&mut tx, &lease.id, phase, reason, delay).await?;
                    "not_dispatched"
                }
                Completion::Submitted(Ok(PreparationSubmission::Accepted { status, .. })) => {
                    publish_status(&mut tx, lease, status, fenced).await?
                }
                Completion::Submitted(Ok(PreparationSubmission::NotSubmitted { .. })) => {
                    "not_submitted"
                }
                Completion::Submitted(_) => "submission_unknown",
                Completion::Polled(Ok(Some(status))) => {
                    publish_status(&mut tx, lease, status, fenced).await?
                }
                Completion::Polled(Ok(None)) => {
                    sqlx::query(
                        "UPDATE source_preparations SET next_check_at = unixepoch() WHERE id = ?",
                    )
                    .bind(&lease.id)
                    .execute(&mut *tx)
                    .await?;
                    "poll_cancelled"
                }
                Completion::Polled(Err(_)) => {
                    let delay = match lease.poll_failures {
                        0 => 5,
                        1 => 15,
                        2 => 60,
                        _ => 300,
                    };
                    sqlx::query("UPDATE source_preparations SET phase = 'preparing',reason = 'provider_unavailable',
                        next_check_at = unixepoch() + ?,poll_failures = poll_failures + 1 WHERE id = ?")
                        .bind(delay).bind(&lease.id).execute(&mut *tx).await?;
                    "poll_retry"
                }
            }
        };
        sqlx::query("UPDATE source_preparations SET lease_token = NULL,lease_until = NULL,updated_at = unixepoch()
            WHERE id = ? AND lease_token = ?")
            .bind(&lease.id).bind(&lease.lease_token).execute(&mut *tx).await?;
        tx.commit().await?;
        Ok(outcome)
    }
}

async fn publish_status(
    connection: &mut SqliteConnection,
    lease: &Lease,
    status: PreparationStatus,
    fenced: bool,
) -> Result<&'static str> {
    let (phase, progress, eta, reason, delay) = match status {
        PreparationStatus::Ready {} if fenced => ("preparing", None, None, None, Some(5)),
        PreparationStatus::Ready {} => {
            let changed = sqlx::query(
                "UPDATE sources SET ready = 1 WHERE id = ?
                AND EXISTS(SELECT 1 FROM current_source_preparation_authority WHERE id = ?)",
            )
            .bind(&lease.source_id)
            .bind(&lease.id)
            .execute(&mut *connection)
            .await?
            .rows_affected();
            if changed != 1 {
                return Err(Error::Conflict(
                    "This prepared source is no longer current".into(),
                ));
            }
            ("ready", Some(10000), None, None, None)
        }
        PreparationStatus::Queued { poll_after_seconds } => (
            "preparing",
            None,
            None,
            None,
            Some(i64::from(poll_after_seconds)),
        ),
        PreparationStatus::Preparing {
            progress_basis_points,
            eta_seconds,
            poll_after_seconds,
        } => (
            "preparing",
            progress_basis_points.map(i64::from),
            eta_seconds.map(i64::from),
            None,
            Some(i64::from(poll_after_seconds)),
        ),
        PreparationStatus::Blocked {
            reason,
            retry_after_seconds,
        } => (
            "blocked",
            None,
            None,
            Some(reason_code(reason)),
            retry_after_seconds.map(i64::from),
        ),
        PreparationStatus::Failed { reason } => {
            ("failed", None, None, Some(reason_code(reason)), None)
        }
        PreparationStatus::SubmissionUnknown {} => ("submission_unknown", None, None, None, None),
    };
    sqlx::query("UPDATE source_preparations SET phase = ?,progress_basis_points = ?,eta_seconds = ?,reason = ?,
        next_check_at = CASE WHEN ? IS NULL THEN NULL ELSE unixepoch() + ? END,poll_failures = 0 WHERE id = ?")
        .bind(phase).bind(progress).bind(eta).bind(reason).bind(delay).bind(delay).bind(&lease.id)
        .execute(connection).await?;
    Ok(phase)
}

async fn schedule(
    connection: &mut SqliteConnection,
    preparation: &str,
    phase: &str,
    reason: Option<&str>,
    delay: Option<i64>,
) -> Result<()> {
    sqlx::query(
        "UPDATE source_preparations SET phase = ?,reason = ?,
        next_check_at = CASE WHEN ? IS NULL THEN NULL ELSE unixepoch() + ? END WHERE id = ?",
    )
    .bind(phase)
    .bind(reason)
    .bind(delay)
    .bind(delay)
    .bind(preparation)
    .execute(connection)
    .await?;
    Ok(())
}

fn reason_code(reason: PreparationReason) -> &'static str {
    match reason {
        PreparationReason::AuthorizationRequired => "authorization_required",
        PreparationReason::QuotaReached => "quota_reached",
        PreparationReason::RateLimited => "rate_limited",
        PreparationReason::ProviderRestricted => "provider_restricted",
        PreparationReason::SourceUnavailable => "source_unavailable",
        PreparationReason::SourceChanged => "source_changed",
        PreparationReason::Unsupported => "unsupported",
        PreparationReason::ProviderUnavailable => "provider_unavailable",
        PreparationReason::InvalidResponse => "invalid_response",
    }
}
