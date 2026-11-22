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
    let recording = recording.to_string();
    recording_access(tx, actor, &recording).await?;
    let (previous, duration): (Option<i64>, i64) = sqlx::query_as(
        "SELECT (SELECT position_ms FROM listening_progress WHERE user_id=?1 AND recording_id=?2),duration_ms
         FROM recordings WHERE id=?2",
    )
    .bind(actor.id())
    .bind(&recording)
    .fetch_one(&mut **tx)
    .await?;
    let played_through =
        !reaches_the_end(previous.unwrap_or(0), duration) && reaches_the_end(position, duration);
    sqlx::query(
        "INSERT INTO listening_progress(user_id, recording_id, position_ms, speed)
         VALUES (?, ?, ?, ?)
         ON CONFLICT(user_id, recording_id) DO UPDATE SET
             position_ms = excluded.position_ms,
             speed = excluded.speed,
             updated_at = unixepoch()",
    )
    .bind(actor.id())
    .bind(&recording)
    .bind(position)
    .bind(speed)
    .execute(&mut **tx)
    .await?;
    let recorded = sqlx::query(
        "INSERT INTO listening_history(user_id, recording_id, play_count)
         SELECT ?1, ?2, ?3
         WHERE COALESCE((SELECT history_enabled FROM account_history_preferences WHERE user_id = ?1), 1) = 1
         ON CONFLICT(user_id, recording_id) DO UPDATE SET played_at = unixepoch(),
             play_count = play_count + excluded.play_count",
    )
    .bind(actor.id())
    .bind(&recording)
    .bind(i64::from(played_through))
    .execute(&mut **tx)
    .await?
    .rows_affected();
    if played_through {
        let copy_requested =
            crate::downloads::request_copy_after_play_on(tx, actor.id(), &recording).await?;
        tracing::debug!(
            event = "recording_played_through",
            history_recorded = recorded > 0,
            copy_requested,
            "Recording played through"
        );
    }
    tracing::trace!(history_recorded = recorded > 0, "Listening progress stored");
    Ok(())
}

fn reaches_the_end(position_ms: i64, duration_ms: i64) -> bool {
    duration_ms > 0 && position_ms.saturating_mul(10) >= duration_ms.saturating_mul(9)
}
