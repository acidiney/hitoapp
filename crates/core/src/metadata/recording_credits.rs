use crate::Result;
use sqlx::{QueryBuilder, Sqlite, Transaction};

pub(super) enum Scope<'a> {
    Recording(&'a str),
    Folder(&'a str),
    Library { user: &'a str, library: &'a str },
}

fn targets(scope: &Scope<'_>) -> QueryBuilder<'static, Sqlite> {
    let mut query = QueryBuilder::new(
        "WITH targets AS (SELECT access.user_id, r.library_id, r.id AS recording_id,
            COALESCE(p.override_artist, r.artist) AS name
         FROM recordings r JOIN library_members access ON access.library_id = r.library_id
         LEFT JOIN personal_recording_metadata p ON p.user_id = access.user_id AND p.recording_id = r.id WHERE 1",
    );
    match scope {
        Scope::Recording(recording) => {
            query.push(" AND r.id = ").push_bind(recording.to_string());
        }
        Scope::Folder(folder) => {
            query.push(" AND r.id IN (SELECT s.recording_id FROM local_files f JOIN sources s ON s.id = f.source_id WHERE f.folder_id = ")
                .push_bind(folder.to_string()).push(")");
        }
        Scope::Library { user, library } => {
            query
                .push(" AND access.user_id = ")
                .push_bind(user.to_string())
                .push(" AND r.library_id = ")
                .push_bind(library.to_string());
        }
    }
    query.push(") ");
    query
}

fn retirement_targets(scope: &Scope<'_>) -> QueryBuilder<'static, Sqlite> {
    let mut query = QueryBuilder::new(
        "WITH targets AS MATERIALIZED (SELECT choice.user_id, r.id AS recording_id
         FROM recordings r JOIN recording_artist_identity_choices choice ON choice.recording_id=r.id
         WHERE choice.retired=0 AND choice.association_id IS NOT NULL",
    );
    match scope {
        Scope::Recording(recording) => {
            query.push(" AND r.id = ").push_bind(recording.to_string());
        }
        Scope::Folder(folder) => {
            query.push(" AND r.id IN (SELECT s.recording_id FROM local_files f JOIN sources s ON s.id = f.source_id WHERE f.folder_id = ")
                .push_bind(folder.to_string()).push(")");
        }
        Scope::Library { user, library } => {
            query
                .push(" AND choice.user_id = ")
                .push_bind(user.to_string())
                .push(" AND r.library_id = ")
                .push_bind(library.to_string());
        }
    }
    query.push(") ");
    query
}

#[tracing::instrument(level = "trace", skip_all, err(Debug))]
pub(super) async fn refresh(tx: &mut Transaction<'_, Sqlite>, scope: Scope<'_>) -> Result<()> {
    let mut register = targets(&scope);
    register.push(super::projection::REGISTER_ARTISTS);
    register.build().execute(&mut **tx).await?;
    let mut bind = targets(&scope);
    bind.push("INSERT INTO account_recording_credits(user_id, recording_id, artist_id)
        SELECT t.user_id, t.recording_id, c.id FROM targets t
        JOIN account_artists c ON c.user_id = t.user_id AND c.library_id = t.library_id AND c.name = t.name COLLATE BINARY WHERE 1
        ON CONFLICT(user_id, recording_id) DO UPDATE SET artist_id = excluded.artist_id
        WHERE artist_id IS NOT excluded.artist_id");
    let changed = bind.build().execute(&mut **tx).await?.rows_affected();
    crate::catalog::artist_catalog::identity::retire_changed(tx, retirement_targets(&scope))
        .await?;
    tracing::trace!(changed, "Account recording credits refreshed");
    Ok(())
}

pub(crate) async fn recording(tx: &mut Transaction<'_, Sqlite>, recording: &str) -> Result<()> {
    refresh(tx, Scope::Recording(recording)).await
}

pub(crate) async fn folder(tx: &mut Transaction<'_, Sqlite>, folder: &str) -> Result<()> {
    refresh(tx, Scope::Folder(folder)).await
}
