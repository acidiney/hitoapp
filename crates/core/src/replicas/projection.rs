use crate::{Action, Error, Operation, Result};
use sqlx::{Sqlite, Transaction};
use uuid::Uuid;

pub(super) fn playlist(operation: &Operation) -> Result<Uuid> {
    match &operation.action {
        Action::PlaylistCreate { playlist_id, name }
        | Action::PlaylistRename { playlist_id, name } => {
            if name.trim().is_empty() || name.len() > 256 {
                return Err(Error::Invalid(
                    "Playlist names must contain 1 to 256 bytes".into(),
                ));
            }
            Ok(*playlist_id)
        }
        Action::PlaylistDelete { playlist_id } => Ok(*playlist_id),
        _ => Err(Error::OperationRejected(
            "This replica currently supports empty playlist changes only".into(),
        )),
    }
}

fn apply(state: &mut Option<(String, bool)>, operation: &Operation) -> Result<()> {
    match &operation.action {
        Action::PlaylistCreate { name, .. } => {
            if state.is_none() {
                *state = Some((name.trim().to_owned(), false));
            }
        }
        Action::PlaylistRename { name, .. } => {
            let (current, deleted) = state.as_mut().ok_or(Error::NotFound)?;
            if !*deleted {
                *current = name.trim().to_owned();
            }
        }
        Action::PlaylistDelete { .. } => {
            let (_, deleted) = state.get_or_insert_with(|| (String::new(), true));
            *deleted = true;
        }
        _ => {
            return Err(Error::OperationRejected(
                "This replica cannot project this server event yet".into(),
            ));
        }
    }
    Ok(())
}

pub(super) async fn accept(
    tx: &mut Transaction<'_, Sqlite>,
    operation: &Operation,
) -> Result<Uuid> {
    let id = playlist(operation)?;
    let mut state: Option<(String, bool)> =
        sqlx::query_as("SELECT name,deleted FROM replica_base_playlists WHERE id=?")
            .bind(id.to_string())
            .fetch_optional(&mut **tx)
            .await?;
    apply(&mut state, operation)?;
    let (name, deleted) = state.ok_or(Error::NotFound)?;
    sqlx::query("INSERT INTO replica_base_playlists(id,name,deleted) VALUES(?,?,?) ON CONFLICT(id) DO UPDATE SET name=excluded.name,deleted=excluded.deleted")
        .bind(id.to_string()).bind(name).bind(deleted).execute(&mut **tx).await?;
    Ok(id)
}

pub(super) async fn append(
    tx: &mut Transaction<'_, Sqlite>,
    account: &str,
    operation: &Operation,
) -> Result<()> {
    let id = playlist(operation)?;
    let mut state: Option<(String, bool)> =
        sqlx::query_as("SELECT name,deleted FROM playlists WHERE id=? AND user_id=?")
            .bind(id.to_string())
            .bind(account)
            .fetch_optional(&mut **tx)
            .await?;
    apply(&mut state, operation)?;
    write(tx, account, id, state).await
}

async fn write(
    tx: &mut Transaction<'_, Sqlite>,
    account: &str,
    id: Uuid,
    state: Option<(String, bool)>,
) -> Result<()> {
    let (name, deleted) = state.ok_or(Error::NotFound)?;
    let changed=sqlx::query("INSERT INTO playlists(id,user_id,name,deleted) VALUES(?,?,?,?) ON CONFLICT(id) DO UPDATE SET name=excluded.name,deleted=excluded.deleted WHERE playlists.user_id=excluded.user_id")
        .bind(id.to_string()).bind(account).bind(name).bind(deleted).execute(&mut **tx).await?.rows_affected();
    if changed != 1 {
        return Err(Error::Conflict(
            "The replica playlist belongs to another profile".into(),
        ));
    }
    Ok(())
}

pub(super) async fn rebuild(
    tx: &mut Transaction<'_, Sqlite>,
    account: &str,
    id: Uuid,
) -> Result<()> {
    let mut state: Option<(String, bool)> =
        sqlx::query_as("SELECT name,deleted FROM replica_base_playlists WHERE id=?")
            .bind(id.to_string())
            .fetch_optional(&mut **tx)
            .await?;
    let pending: Vec<String> = sqlx::query_scalar(
        "SELECT payload FROM replica_outbox WHERE playlist_id=? AND echoed=0 ORDER BY ordinal",
    )
    .bind(id.to_string())
    .fetch_all(&mut **tx)
    .await?;
    for payload in pending {
        let operation: Operation = serde_json::from_str(&payload)
            .map_err(|_| Error::Internal("Stored replica operation is invalid".into()))?;
        apply(&mut state, &operation)?;
    }
    write(tx, account, id, state).await
}
