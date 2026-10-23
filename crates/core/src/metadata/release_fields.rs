use super::{TextField, validate_text};
use crate::{Actor, Error, Result};
use sqlx::{QueryBuilder, Sqlite, Transaction};
use uuid::Uuid;

#[tracing::instrument(level = "debug", skip_all, err(Debug), fields(account_id = actor.id(), operation_id = %operation, inherited = value.is_none()))]
pub(crate) async fn set_release_field(
    tx: &mut Transaction<'_, Sqlite>,
    actor: &Actor,
    release: &Uuid,
    field: TextField,
    value: Option<&str>,
    operation: &Uuid,
) -> Result<()> {
    let release = release.to_string();
    let allowed: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM releases a
        JOIN library_members m ON m.library_id = a.library_id AND m.user_id = ? WHERE a.id = ?)",
    )
    .bind(actor.id())
    .bind(&release)
    .fetch_one(&mut **tx)
    .await?;
    if !allowed {
        return Err(Error::NotFound);
    }
    validate_text(value)?;
    let (title, artist, title_operation, artist_operation) = match field {
        TextField::Title => (value, None, Some(operation.to_string()), None),
        TextField::Artist => (None, value, None, Some(operation.to_string())),
    };
    let mut query = QueryBuilder::new(
        "INSERT INTO personal_release_metadata(user_id, release_id, override_title, override_artist, title_operation_id, artist_operation_id) VALUES (",
    );
    query.push_bind(actor.id().to_owned()).push(", ").push_bind(release.clone()).push(", ")
        .push_bind(title.map(str::to_owned)).push(", ").push_bind(artist.map(str::to_owned))
        .push(", ").push_bind(title_operation).push(", ").push_bind(artist_operation).push(")")
        .push(" ON CONFLICT(user_id, release_id) DO UPDATE SET ").push(match field {
            TextField::Title => "override_title = excluded.override_title, title_operation_id = excluded.title_operation_id",
            TextField::Artist => "override_artist = excluded.override_artist, artist_operation_id = excluded.artist_operation_id",
        });
    query.build().execute(&mut **tx).await?;
    super::projection::release(tx, actor.id(), &release).await?;
    tracing::debug!("Personal release metadata accepted");
    Ok(())
}
