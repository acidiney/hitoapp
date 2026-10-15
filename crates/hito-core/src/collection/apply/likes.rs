use super::Transaction;
use crate::{Actor, Result, identity::recording_access};
use uuid::Uuid;

pub(super) async fn set(
    tx: &mut Transaction<'_>,
    actor: &Actor,
    recording: &Uuid,
    liked: bool,
) -> Result<()> {
    recording_access(tx, actor, &recording.to_string()).await?;
    sqlx::query(
        "INSERT INTO likes VALUES (?, ?, ?)
         ON CONFLICT(user_id, recording_id) DO UPDATE SET liked = excluded.liked",
    )
    .bind(actor.id())
    .bind(recording.to_string())
    .bind(liked)
    .execute(&mut **tx)
    .await?;
    Ok(())
}
