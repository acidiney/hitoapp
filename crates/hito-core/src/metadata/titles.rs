use crate::{Actor, Error, Result, identity::recording_access};
use sqlx::{QueryBuilder, Sqlite, Transaction};
use uuid::Uuid;

pub(crate) const RECORDING_TITLE: &str = "COALESCE(personal_title.override_title, r.title)";
pub(crate) const APPEARANCE_TITLE: &str = "COALESCE(personal_title.override_title, m.title)";

pub(crate) fn join_title(query: &mut QueryBuilder<'_, Sqlite>, actor: &Actor) {
    query.push(" LEFT JOIN personal_recording_metadata personal_title ON personal_title.recording_id = r.id AND personal_title.user_id = ")
        .push_bind(actor.id().to_owned());
}

#[tracing::instrument(level = "debug", skip_all, err(Debug), fields(account_id = actor.id(), operation_id = %operation, inherited = title.is_none()))]
pub(crate) async fn set_title(
    tx: &mut Transaction<'_, Sqlite>,
    actor: &Actor,
    recording: &Uuid,
    title: Option<&str>,
    operation: &Uuid,
) -> Result<()> {
    let recording = recording.to_string();
    recording_access(tx, actor, &recording).await?;
    if let Some(title) = title
        && (title.trim().is_empty() || title.len() > 2048 || title.chars().any(char::is_control))
    {
        return Err(Error::Invalid(
            "Use a title of 1 to 2048 bytes without control characters".into(),
        ));
    }
    sqlx::query(
        "INSERT INTO personal_recording_metadata(user_id, recording_id, override_title, title, artist, title_operation_id)
         SELECT ?, r.id, ?, COALESCE(?, r.title), r.artist, ? FROM recordings r WHERE r.id = ?
         ON CONFLICT(user_id, recording_id) DO UPDATE SET override_title = excluded.override_title,
            title = excluded.title, artist = excluded.artist, title_operation_id = excluded.title_operation_id",
    )
    .bind(actor.id()).bind(title).bind(title).bind(operation.to_string()).bind(&recording)
    .execute(&mut **tx).await?;
    tracing::debug!("Personal title accepted");
    Ok(())
}
