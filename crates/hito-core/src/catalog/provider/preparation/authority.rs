use crate::{Error, Result};
use sqlx::SqliteConnection;

#[derive(sqlx::FromRow)]
pub(super) struct Target {
    pub library_id: String,
    pub access_grant: String,
    pub binding_id: String,
    pub binding_revision: i64,
    pub provider_recording_id: String,
    pub remote_source_id: String,
    pub installation_id: String,
    pub ready: bool,
}

pub(super) async fn target(
    connection: &mut SqliteConnection,
    account: &str,
    recording: &str,
    source: &str,
) -> Result<Target> {
    sqlx::query_as("SELECT recording.library_id,access.id AS access_grant,
        binding.id AS binding_id,binding.revision AS binding_revision,binding.provider_recording_id,
        remote.provider_source_id AS remote_source_id,remote.installation_id,source.ready
        FROM sources source JOIN recordings recording ON recording.id = source.recording_id
        JOIN library_access_grants access ON access.library_id = recording.library_id AND access.user_id = ?
        JOIN provider_sources remote ON remote.source_id = source.id AND remote.recording_id = recording.id
        JOIN provider_items binding ON binding.id = remote.binding_id AND binding.retired = 0
            AND binding.recording_id = recording.id AND binding.installation_id = remote.installation_id
        JOIN provider_source_state snapshot ON snapshot.binding_id = binding.id AND snapshot.generation = remote.generation
        JOIN plugin_installations installation ON installation.id = remote.installation_id AND installation.user_id = ?
        WHERE source.id = ? AND source.recording_id = ?")
        .bind(account).bind(account).bind(source).bind(recording)
        .fetch_optional(connection).await?.ok_or(Error::NotFound)
}

pub(super) async fn check_on(connection: &mut SqliteConnection, preparation: &str) -> Result<()> {
    let current: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM current_source_preparation_authority WHERE id = ?)",
    )
    .bind(preparation)
    .fetch_one(connection)
    .await?;
    if current {
        Ok(())
    } else {
        Err(Error::Conflict(
            "This source preparation authority changed".into(),
        ))
    }
}

pub(super) async fn cancel_stale_on(
    connection: &mut SqliteConnection,
    preparation: &str,
) -> Result<()> {
    let cancelled = sqlx::query("UPDATE source_preparations SET authority_cancelled = 1,
        phase = CASE WHEN submission_state IN ('submitting','unknown') THEN 'submission_unknown' ELSE 'cancelled' END,
        submission_state = CASE WHEN submission_state = 'submitting' THEN 'unknown' ELSE submission_state END,
        next_check_at = NULL,lease_token = NULL,lease_until = NULL,updated_at = unixepoch()
        WHERE id = ? AND authority_cancelled = 0
            AND NOT EXISTS(SELECT 1 FROM current_source_preparation_authority authority WHERE authority.id = source_preparations.id)")
        .bind(preparation).execute(connection).await?.rows_affected();
    if cancelled > 0 {
        tracing::debug!(
            event = "source_preparation_cancelled",
            preparation_id = preparation,
            outcome = "authority_changed",
            "Source preparation authority cancelled"
        );
    }
    Ok(())
}
