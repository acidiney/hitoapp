use super::{Catalog, Recording, Release, recordings_on, releases::releases_among_ids_on};
use crate::{Actor, Error, Result};
use serde::Serialize;
use sqlx::QueryBuilder;
use utoipa::ToSchema;
use uuid::Uuid;

const GENRE_TILES: i64 = 18;
const GENRE_ALBUMS: i64 = 24;
const GENRE_TRACKS: i64 = 100;
const GENRE_COUNT_CAP: i64 = 10_000;
const MAX_NAME_CHARS: usize = 64;

#[derive(Serialize, sqlx::FromRow, ToSchema)]
pub struct Genre {
    pub key: String,
    pub label: String,
    pub track_count: i64,
}

#[derive(Serialize, ToSchema)]
pub struct GenreDetail {
    pub genre: Genre,
    pub albums: Vec<Release>,
    pub tracks: Vec<Recording>,
}

/// The stable key and display label of a genre name: spacing collapsed, case ignored for the
/// key, both bounded. Names with no letters or digits have no identity.
pub(crate) fn genre_identity(name: &str) -> Option<(String, String)> {
    let label: String = name
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(MAX_NAME_CHARS)
        .collect();
    let label = label.trim().to_owned();
    if !label.chars().any(char::is_alphanumeric) {
        return None;
    }
    Some((
        label.to_lowercase().chars().take(MAX_NAME_CHARS).collect(),
        label,
    ))
}

/// Library recordings of one genre among the account's libraries, one row per recording.
const IN_GENRE: &str = " FROM source_genres g
    JOIN sources s ON s.id = g.source_id
    JOIN recordings r ON r.id = s.recording_id
    JOIN library_members m ON m.library_id = r.library_id AND m.user_id = ";
const NOT_PODCAST: &str =
    " AND NOT EXISTS (SELECT 1 FROM podcast_episodes e WHERE e.recording_id = r.id)";

impl Catalog {
    #[tracing::instrument(level = "debug", skip_all, err(Debug), fields(account_id = actor.id()))]
    pub async fn genres(&self, actor: &Actor) -> Result<Vec<Genre>> {
        let mut query = QueryBuilder::new(
            "SELECT g.genre_key AS key, min(g.label) AS label, count(DISTINCT r.id) AS track_count",
        );
        query
            .push(IN_GENRE)
            .push_bind(actor.id().to_owned())
            .push(" WHERE 1 = 1")
            .push(NOT_PODCAST)
            .push(" GROUP BY g.genre_key ORDER BY track_count DESC, g.genre_key LIMIT ")
            .push_bind(GENRE_TILES);
        let mut tx = self.store.pool.begin().await?;
        actor.check_session_on(&mut tx).await?;
        let genres: Vec<Genre> = query.build_query_as().fetch_all(&mut *tx).await?;
        tx.commit().await?;
        tracing::debug!(tiles = genres.len(), "Genre tiles read");
        Ok(genres)
    }

    #[tracing::instrument(level = "debug", skip_all, err(Debug), fields(account_id = actor.id()))]
    pub async fn genre(&self, actor: &Actor, key: &str) -> Result<GenreDetail> {
        let (key, _) = genre_identity(key).ok_or(Error::NotFound)?;
        let mut tx = self.store.pool.begin().await?;
        actor.check_session_on(&mut tx).await?;
        let mut head = QueryBuilder::new(
            "SELECT min(label), count(*) FROM (SELECT min(g.label) AS label, r.id",
        );
        head.push(IN_GENRE)
            .push_bind(actor.id().to_owned())
            .push(" WHERE g.genre_key = ")
            .push_bind(key.clone())
            .push(NOT_PODCAST)
            .push(" GROUP BY r.id LIMIT ")
            .push_bind(GENRE_COUNT_CAP)
            .push(")");
        let (label, track_count): (Option<String>, i64) =
            head.build_query_as().fetch_one(&mut *tx).await?;
        let Some(label) = label else {
            return Err(Error::NotFound);
        };
        let genre = Genre {
            key: key.clone(),
            label,
            track_count,
        };
        let mut ids = QueryBuilder::new("SELECT r.id");
        ids.push(IN_GENRE)
            .push_bind(actor.id().to_owned())
            .push(" WHERE g.genre_key = ")
            .push_bind(key.clone())
            .push(NOT_PODCAST)
            .push(" GROUP BY r.id ORDER BY r.title COLLATE NOCASE, r.id LIMIT ")
            .push_bind(GENRE_TRACKS);
        let track_ids: Vec<String> = ids.build_query_scalar().fetch_all(&mut *tx).await?;
        let track_ids = track_ids
            .iter()
            .map(|id| {
                Uuid::parse_str(id)
                    .map_err(|_| Error::Internal("Stored recording identity is invalid".into()))
            })
            .collect::<Result<Vec<_>>>()?;
        let tracks = recordings_on(&mut tx, &self.host, actor, &track_ids).await?;
        let mut albums = QueryBuilder::new("SELECT r.release_id");
        albums
            .push(IN_GENRE)
            .push_bind(actor.id().to_owned())
            .push(" WHERE g.genre_key = ")
            .push_bind(key)
            .push(NOT_PODCAST)
            .push(" AND r.release_id IS NOT NULL GROUP BY r.release_id ORDER BY count(DISTINCT r.id) DESC, r.release_id LIMIT ")
            .push_bind(GENRE_ALBUMS);
        let order: Vec<String> = albums.build_query_scalar().fetch_all(&mut *tx).await?;
        let mut found = releases_among_ids_on(&mut tx, actor, &order).await?;
        found.sort_by_key(|release| order.iter().position(|id| *id == release.id));
        actor.check_session_on(&mut tx).await?;
        tx.commit().await?;
        tracing::debug!(
            tracks = tracks.len(),
            albums = found.len(),
            "Genre page read"
        );
        Ok(GenreDetail {
            genre,
            albums: found,
            tracks,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::genre_identity;

    #[test]
    fn identity_ignores_case_and_spacing_and_rejects_names_without_letters() {
        assert_eq!(
            genre_identity("  Hip   Hop "),
            Some(("hip hop".into(), "Hip Hop".into()))
        );
        assert_eq!(
            genre_identity("Fado").unwrap().0,
            genre_identity("FADO").unwrap().0
        );
        assert_eq!(genre_identity(" - "), None);
        assert_eq!(genre_identity(""), None);
        assert_eq!(
            genre_identity(&"a".repeat(200)).unwrap().1.chars().count(),
            64
        );
    }
}
