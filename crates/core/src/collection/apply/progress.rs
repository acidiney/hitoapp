use super::Transaction;
use crate::{Actor, Error, Result, identity::recording_access};
use uuid::Uuid;

pub(super) async fn set(
    tx: &mut Transaction<'_>,
    actor: &Actor,
    recording: &Uuid,
    position: i64,
    speed: f64,
) -> Result<()> {
    if position < 0 || !speed.is_finite() || !(0.25..=4.0).contains(&speed) {
        return Err(Error::Invalid(
            "Use a nonnegative position and a speed between 0.25 and 4".into(),
        ));
    }
    recording_access(tx, actor, &recording.to_string()).await?;
    sqlx::query(
        "INSERT INTO listening_progress(user_id, recording_id, position_ms, speed)
         VALUES (?, ?, ?, ?)
         ON CONFLICT(user_id, recording_id) DO UPDATE SET
             position_ms = excluded.position_ms,
             speed = excluded.speed,
             updated_at = unixepoch()",
    )
    .bind(actor.id())
    .bind(recording.to_string())
    .bind(position)
    .bind(speed)
    .execute(&mut **tx)
    .await?;
    let recorded = sqlx::query(
        "INSERT INTO listening_history(user_id, recording_id)
         SELECT ?1, ?2
         WHERE COALESCE((SELECT history_enabled FROM account_history_preferences WHERE user_id = ?1), 1) = 1
         ON CONFLICT(user_id, recording_id) DO UPDATE SET played_at = unixepoch()",
    )
    .bind(actor.id())
    .bind(recording.to_string())
    .execute(&mut **tx)
    .await?
    .rows_affected();
    tracing::trace!(history_recorded = recorded > 0, "Listening progress stored");
    Ok(())
}
