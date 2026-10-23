use crate::Result;
use sqlx::SqliteConnection;

const MAX_UNREFERENCED_JOBS: i64 = 32;

pub(in crate::collection) async fn prune(
    connection: &mut SqliteConnection,
    playlist: &str,
) -> Result<u64> {
    let pruned = sqlx::query("DELETE FROM playlist_follow_jobs WHERE rowid IN (
        SELECT job.rowid FROM playlist_follow_jobs job
        WHERE job.playlist_id = ? AND job.status IN ('complete','failed','cancelled')
            AND NOT EXISTS(SELECT 1 FROM playlist_follow_refresh_receipts receipt WHERE receipt.job_id = job.id)
        ORDER BY job.created_at DESC,job.rowid DESC LIMIT -1 OFFSET ?)")
        .bind(playlist)
        .bind(MAX_UNREFERENCED_JOBS)
        .execute(connection)
        .await?
        .rows_affected();
    if pruned > 0 {
        tracing::debug!(
            pruned_jobs = pruned,
            "Unreferenced follow refresh history trimmed"
        );
    }
    Ok(pruned)
}
