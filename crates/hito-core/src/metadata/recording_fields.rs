use super::validate_text;
use crate::{Actor, Result, identity::recording_access};
use sqlx::{QueryBuilder, Sqlite, Transaction};
use uuid::Uuid;

pub(crate) const RECORDING_TITLE: &str = "COALESCE(personal_metadata.override_title, r.title)";
pub(crate) const RECORDING_ARTIST: &str = "COALESCE(personal_metadata.override_artist, r.artist)";
pub(crate) const APPEARANCE_ARTIST: &str = "COALESCE(personal_metadata.override_artist, m.artist)";
pub(crate) const APPEARANCE_TITLE: &str = "COALESCE(personal_metadata.override_title, m.title)";

pub(crate) fn join_recording_fields(query: &mut QueryBuilder<'_, Sqlite>, actor: &Actor) {
    query.push(" LEFT JOIN personal_recording_metadata personal_metadata ON personal_metadata.recording_id = r.id AND personal_metadata.user_id = ")
        .push_bind(actor.id().to_owned());
}

#[derive(Clone, Copy)]
pub(crate) enum TextField {
    Title,
    Artist,
}

#[tracing::instrument(level = "debug", skip_all, err(Debug), fields(account_id = actor.id(), operation_id = %operation, inherited = value.is_none()))]
pub(crate) async fn set_recording_field(
    tx: &mut Transaction<'_, Sqlite>,
    actor: &Actor,
    recording: &Uuid,
    field: TextField,
    value: Option<&str>,
    operation: &Uuid,
) -> Result<()> {
    let recording = recording.to_string();
    recording_access(tx, actor, &recording).await?;
    validate_text(value)?;
    let (title, artist, title_operation, artist_operation) = match field {
        TextField::Title => (value, None, Some(operation.to_string()), None),
        TextField::Artist => (None, value, None, Some(operation.to_string())),
    };
    let mut query = QueryBuilder::new(
        "INSERT INTO personal_recording_metadata(user_id, recording_id, override_title, override_artist, title, artist, title_operation_id, artist_operation_id) SELECT ",
    );
    query.push_bind(actor.id().to_owned()).push(", r.id, ").push_bind(title.map(str::to_owned))
        .push(", ").push_bind(artist.map(str::to_owned)).push(", COALESCE(").push_bind(title.map(str::to_owned))
        .push(", r.title), COALESCE(").push_bind(artist.map(str::to_owned)).push(", r.artist), ")
        .push_bind(title_operation).push(", ").push_bind(artist_operation)
        .push(" FROM recordings r WHERE r.id = ").push_bind(recording.clone())
        .push(" ON CONFLICT(user_id, recording_id) DO UPDATE SET ").push(match field {
            TextField::Title => "override_title = excluded.override_title, title = excluded.title, title_operation_id = excluded.title_operation_id",
            TextField::Artist => "override_artist = excluded.override_artist, artist = excluded.artist, artist_operation_id = excluded.artist_operation_id",
        });
    query.build().execute(&mut **tx).await?;
    if matches!(field, TextField::Artist) {
        super::projection::recording(tx, actor.id(), &recording).await?;
    }
    tracing::debug!("Personal recording metadata accepted");
    Ok(())
}
