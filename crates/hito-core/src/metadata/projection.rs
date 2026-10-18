use crate::Result;
use sqlx::{QueryBuilder, Sqlite, Transaction};

pub(super) const REGISTER_ARTISTS: &str = "INSERT INTO account_artists(user_id, id, library_id, name)
        SELECT t.user_id, COALESCE(shared.id, lower(hex(randomblob(16)))), t.library_id, t.name FROM targets t
        LEFT JOIN artists shared ON shared.library_id = t.library_id AND shared.name = t.name COLLATE BINARY WHERE t.name IS NOT NULL
        ON CONFLICT(user_id, library_id, name) DO NOTHING";

enum Scope<'a> {
    Source(&'a str),
    Release { user: &'a str, release: &'a str },
    Recording { user: &'a str, recording: &'a str },
    Library { user: &'a str, library: &'a str },
}

impl Scope<'_> {
    fn filter(&self, query: &mut QueryBuilder<'_, Sqlite>, releases: bool) {
        match self {
            Self::Source(source) => {
                query
                    .push(if releases {
                        " AND a.id = (SELECT release_id FROM source_metadata WHERE source_id = "
                    } else {
                        " AND m.source_id = "
                    })
                    .push_bind(source.to_string());
                if releases {
                    query.push(")");
                }
            }
            Self::Release { user, release } => {
                query
                    .push(" AND access.user_id = ")
                    .push_bind(user.to_string())
                    .push(if releases {
                        " AND a.id = "
                    } else {
                        " AND m.release_id = "
                    })
                    .push_bind(release.to_string());
            }
            Self::Recording { user, recording } => {
                query.push(" AND access.user_id = ").push_bind(user.to_string())
                    .push(if releases { " AND EXISTS (SELECT 1 FROM source_metadata m JOIN sources s ON s.id = m.source_id WHERE m.release_id = a.id AND s.recording_id = " } else { " AND r.id = " })
                    .push_bind(recording.to_string());
                if releases {
                    query.push(")");
                }
            }
            Self::Library { user, library } => {
                query
                    .push(" AND access.user_id = ")
                    .push_bind(user.to_string())
                    .push(if releases {
                        " AND a.library_id = "
                    } else {
                        " AND r.library_id = "
                    })
                    .push_bind(library.to_string());
            }
        }
    }
}

#[tracing::instrument(level = "debug", skip_all, err(Debug))]
pub(crate) async fn provision(
    tx: &mut Transaction<'_, Sqlite>,
    user: &str,
    library: &str,
) -> Result<()> {
    sqlx::query(
        "INSERT INTO account_artists(user_id, id, library_id, name)
        SELECT ?, c.id, c.library_id, c.name FROM artists c WHERE c.library_id = ?
        ON CONFLICT(user_id, library_id, name) DO NOTHING",
    )
    .bind(user)
    .bind(library)
    .execute(&mut **tx)
    .await?;
    let scope = Scope::Library { user, library };
    releases(tx, &scope).await?;
    credits(tx, &scope).await?;
    super::recording_credits::refresh(
        tx,
        super::recording_credits::Scope::Library { user, library },
    )
    .await?;
    tracing::debug!("Account catalog provisioned");
    Ok(())
}

#[tracing::instrument(level = "trace", skip_all, err(Debug))]
pub(crate) async fn source(tx: &mut Transaction<'_, Sqlite>, source: &str) -> Result<()> {
    sqlx::query(
        "DELETE FROM account_source_credits
         WHERE source_id = ?
           AND NOT EXISTS (
               SELECT 1 FROM source_metadata m
               JOIN sources s ON s.id = m.source_id
               JOIN recordings r ON r.id = s.recording_id
               JOIN releases a ON a.id = m.release_id AND a.library_id = r.library_id
               WHERE m.source_id = account_source_credits.source_id
           )",
    )
    .bind(source)
    .execute(&mut **tx)
    .await?;
    let scope = Scope::Source(source);
    releases(tx, &scope).await?;
    credits(tx, &scope).await?;
    tracing::trace!("Account source metadata refreshed");
    Ok(())
}

#[tracing::instrument(level = "debug", skip_all, err(Debug))]
pub(super) async fn recording(
    tx: &mut Transaction<'_, Sqlite>,
    user: &str,
    recording: &str,
) -> Result<()> {
    credits(tx, &Scope::Recording { user, recording }).await?;
    super::recording_credits::recording(tx, recording).await?;
    tracing::debug!("Account recording credits refreshed");
    Ok(())
}

#[tracing::instrument(level = "debug", skip_all, err(Debug))]
pub(crate) async fn release(
    tx: &mut Transaction<'_, Sqlite>,
    user: &str,
    release: &str,
) -> Result<()> {
    releases(tx, &Scope::Release { user, release }).await?;
    tracing::debug!("Account release metadata refreshed");
    Ok(())
}

fn source_targets(scope: &Scope<'_>) -> QueryBuilder<'static, Sqlite> {
    let mut query = QueryBuilder::new(
        "WITH targets AS (SELECT access.user_id, r.library_id, r.id AS recording_id, m.source_id, a.id AS release_id,
            COALESCE(p.override_artist, m.artist) AS name
         FROM source_metadata m JOIN sources s ON s.id = m.source_id JOIN recordings r ON r.id = s.recording_id
         JOIN releases a ON a.id = m.release_id AND a.library_id = r.library_id
         JOIN library_members access ON access.library_id = r.library_id
         LEFT JOIN personal_recording_metadata p ON p.user_id = access.user_id AND p.recording_id = r.id WHERE 1",
    );
    scope.filter(&mut query, false);
    query.push(") ");
    query
}

async fn credits(tx: &mut Transaction<'_, Sqlite>, scope: &Scope<'_>) -> Result<()> {
    let mut register = source_targets(scope);
    register.push(REGISTER_ARTISTS);
    register.build().execute(&mut **tx).await?;
    let mut bind = source_targets(scope);
    bind.push("INSERT INTO account_source_credits(user_id, source_id, recording_id, release_id, artist_id)
        SELECT t.user_id, t.source_id, t.recording_id, t.release_id, c.id FROM targets t
        JOIN account_artists c ON c.user_id = t.user_id AND c.library_id = t.library_id AND c.name = t.name COLLATE BINARY WHERE 1
        ON CONFLICT(user_id, source_id) DO UPDATE SET recording_id = excluded.recording_id, release_id = excluded.release_id, artist_id = excluded.artist_id
        WHERE recording_id IS NOT excluded.recording_id OR release_id IS NOT excluded.release_id OR artist_id IS NOT excluded.artist_id");
    bind.build().execute(&mut **tx).await?;
    Ok(())
}

fn release_targets(scope: &Scope<'_>) -> QueryBuilder<'static, Sqlite> {
    let mut query = QueryBuilder::new(
        "WITH targets AS (SELECT access.user_id, a.id, a.library_id, COALESCE(p.override_title, a.title) AS title, COALESCE(p.override_artist, a.artist) AS name
         FROM releases a JOIN library_members access ON access.library_id = a.library_id
         LEFT JOIN personal_release_metadata p ON p.user_id = access.user_id AND p.release_id = a.id WHERE 1",
    );
    scope.filter(&mut query, true);
    query.push(") ");
    query
}

async fn releases(tx: &mut Transaction<'_, Sqlite>, scope: &Scope<'_>) -> Result<()> {
    let mut register = release_targets(scope);
    register.push(REGISTER_ARTISTS);
    register.build().execute(&mut **tx).await?;
    let mut bind = release_targets(scope);
    bind.push("INSERT INTO account_releases(user_id, id, library_id, title, artist, artist_id)
        SELECT t.user_id, t.id, t.library_id, t.title, t.name, c.id FROM targets t
        LEFT JOIN account_artists c ON c.user_id = t.user_id AND c.library_id = t.library_id AND c.name = t.name COLLATE BINARY WHERE 1
        ON CONFLICT(user_id, id) DO UPDATE SET title = excluded.title, artist = excluded.artist, artist_id = excluded.artist_id
        WHERE title IS NOT excluded.title OR artist IS NOT excluded.artist OR artist_id IS NOT excluded.artist_id");
    bind.build().execute(&mut **tx).await?;
    Ok(())
}
