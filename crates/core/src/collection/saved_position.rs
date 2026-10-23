use super::{Collection, SavedPosition};
use crate::{Actor, Error, Result};
use uuid::Uuid;

impl Collection {
    #[tracing::instrument(level = "debug", skip_all, fields(account_id = actor.id(), %recording_id))]
    pub async fn saved_position(
        &self,
        actor: &Actor,
        recording_id: Uuid,
    ) -> Result<Option<SavedPosition>> {
        let result = tokio::time::timeout(std::time::Duration::from_secs(1), async {
            actor.check_session(&self.store).await?;
            let position: (Option<i64>,) = sqlx::query_as(
                "SELECT p.position_ms FROM recordings r
                 JOIN library_members m ON m.library_id = r.library_id AND m.user_id = ?
                 LEFT JOIN listening_progress p ON p.recording_id = r.id AND p.user_id = m.user_id
                 WHERE r.id = ?",
            )
            .bind(actor.id())
            .bind(recording_id.to_string())
            .fetch_optional(&self.store.pool)
            .await?
            .ok_or(Error::NotFound)?;
            actor.check_session(&self.store).await?;
            Ok(position.0.map(|position_ms| SavedPosition {
                position_ms: position_ms as u64,
            }))
        })
        .await
        .unwrap_or(Err(Error::Upstream(
            "Saved playback position could not be checked in time",
        )));
        match &result {
            Ok(saved) => tracing::debug!(found = saved.is_some(), "Saved playback position loaded"),
            Err(error) => tracing::debug!(
                error_code = error.code(),
                "Saved playback position rejected"
            ),
        }
        result
    }
}
