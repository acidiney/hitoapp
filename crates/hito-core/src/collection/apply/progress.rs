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
    Ok(())
}
