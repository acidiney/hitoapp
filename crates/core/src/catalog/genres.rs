use super::{
    Catalog, Recording, Release, push_available, recordings_on, releases::releases_among_ids_on,
};
use crate::{Actor, Error, Result};
use serde::Serialize;
use sqlx::QueryBuilder;
use utoipa::ToSchema;
use uuid::Uuid;

const GENRE_TILES: i64 = 18;
const GENRE_ALBUMS: i64 = 24;
const GENRE_TRACKS: i64 = 100;
const GENRE_COUNT_CAP: i64 = 1_000;
const GENRE_SCAN: i64 = 2_000;
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

/// How a genre query names its genre: a bound key, or a column of an enclosing query.
enum Key<'a> {
    Bound(&'a str),
    Column(&'static str),
}

/// Appends one `SELECT * FROM (...)` per genre source over the account's playable library
/// recordings of one genre, joined by `UNION ALL`: `r` is the recording and `g` the genre row.
/// File-tag genres come first and, for a recording that has any, are the only ones used;
/// otherwise the genres MusicBrainz lists for the recording's chosen artist, release group or
/// recording count. Each source stops at `limit` rows, so the work depends on the limit and not
/// on how many tracks a genre has. Podcast episodes, other accounts' libraries and recordings
/// without a playable source never match.
fn genre_recordings(
    query: &mut QueryBuilder<'_, sqlx::Sqlite>,
    host: &crate::HostDeviceId,
    actor: &Actor,
    key: &Key<'_>,
    columns: &str,
    limit: i64,
) {
    for (index, source) in [
        GenreSource::File,
        GenreSource::Artist,
        GenreSource::Identity,
    ]
    .into_iter()
    .enumerate()
    {
        if index > 0 {
            query.push(" UNION ALL ");
        }
        query.push("SELECT * FROM (SELECT ").push(columns);
        match source {
            GenreSource::File => query.push(
                " FROM source_genres g
                JOIN sources s ON s.id = g.source_id
                JOIN recordings r ON r.id = s.recording_id",
            ),
            GenreSource::Artist => query
                .push(
                    " FROM musicbrainz_genres g
                    JOIN account_artist_identity_groups gr ON gr.user_id = ",
                )
                .push_bind(actor.id().to_owned())
                .push(
                    " AND gr.provider_key = 'builtin:musicbrainz' AND gr.provider_artist_id = g.mbid
                    JOIN recording_artist_identity_choices c ON c.user_id = gr.user_id
                        AND c.group_id = gr.id AND c.retired = 0 AND c.association_id IS NOT NULL
                    JOIN recordings r ON r.id = c.recording_id",
                ),
            GenreSource::Identity => query.push(
                " FROM musicbrainz_genres g
                JOIN recording_musicbrainz_observations o ON o.entity = g.entity AND o.mbid = g.mbid
                JOIN recordings r ON r.id = o.recording_id AND r.metadata_observation_id = o.observation_id",
            ),
        };
        query
            .push(" JOIN library_members m ON m.library_id = r.library_id AND m.user_id = ")
            .push_bind(actor.id().to_owned())
            .push(" WHERE g.genre_key = ");
        match key {
            Key::Bound(key) => query.push_bind((*key).to_owned()),
            Key::Column(column) => query.push(*column),
        };
        match source {
            GenreSource::File => {}
            GenreSource::Artist => {
                query
                    .push(" AND g.entity = 'artist' AND ")
                    .push(NO_FILE_GENRE);
            }
            GenreSource::Identity => {
                query
                    .push(" AND g.entity IN ('release_group','recording') AND ")
                    .push(NO_FILE_GENRE)
                    .push(NO_RECORDING_REPEAT);
            }
        }
        query.push(
            " AND NOT EXISTS (SELECT 1 FROM podcast_episodes e WHERE e.recording_id = r.id) AND ",
        );
        push_available(query, host, actor.id());
        if matches!(source, GenreSource::File) && limit > 1 {
            query.push(" GROUP BY r.id");
        }
        query.push(" LIMIT ").push_bind(limit).push(")");
    }
}

#[derive(Clone, Copy)]
enum GenreSource {
    File,
    Artist,
    Identity,
}

/// A release group row is skipped when the recording's own identity names the same genre, so
/// the identity source yields each recording once and its limit counts recordings.
const NO_RECORDING_REPEAT: &str = " AND (g.entity = 'recording' OR NOT EXISTS (
    SELECT 1 FROM recording_musicbrainz_observations o2
    JOIN musicbrainz_genres g2 ON g2.entity = 'recording' AND g2.mbid = o2.mbid
        AND g2.genre_key = g.genre_key
    WHERE o2.observation_id = o.observation_id AND o2.entity = 'recording'))";

const NO_FILE_GENRE: &str = "NOT EXISTS (SELECT 1 FROM sources fs
    JOIN source_genres fg ON fg.source_id = fs.id WHERE fs.recording_id = r.id)";

/// Appends the smallest genre key above `after` that one of the account's own recordings
/// carries in any genre source, so other accounts' keys never use up the key walk.
fn push_next_key(query: &mut QueryBuilder<'_, sqlx::Sqlite>, actor: &Actor, after: &str) {
    let account = actor.id().to_owned();
    query.push("(SELECT min(k) FROM (");
    query
        .push(format!(
            "SELECT (SELECT g.genre_key FROM source_genres g
                JOIN sources s ON s.id = g.source_id
                JOIN recordings r ON r.id = s.recording_id
                JOIN library_members m ON m.library_id = r.library_id AND m.user_id = "
        ))
        .push_bind(account.clone())
        .push(format!(
            " WHERE g.genre_key > {after} ORDER BY g.genre_key LIMIT 1) AS k
            UNION ALL
            SELECT (SELECT g.genre_key FROM musicbrainz_genres g
                JOIN account_artist_identity_groups gr ON gr.user_id = "
        ))
        .push_bind(account.clone())
        .push(format!(
            " AND gr.provider_key = 'builtin:musicbrainz' AND gr.provider_artist_id = g.mbid
                WHERE g.entity = 'artist' AND g.genre_key > {after}
                    AND EXISTS (SELECT 1 FROM recording_artist_identity_choices c
                        WHERE c.user_id = gr.user_id AND c.group_id = gr.id AND c.retired = 0
                            AND c.association_id IS NOT NULL)
                ORDER BY g.genre_key LIMIT 1)
            UNION ALL
            SELECT (SELECT g.genre_key FROM musicbrainz_genres g
                JOIN recording_musicbrainz_observations o ON o.entity = g.entity AND o.mbid = g.mbid
                JOIN recordings r ON r.id = o.recording_id AND r.metadata_observation_id = o.observation_id
                JOIN library_members m ON m.library_id = r.library_id AND m.user_id = "
        ))
        .push_bind(account)
        .push(format!(
            " WHERE g.entity IN ('release_group','recording') AND g.genre_key > {after}
                ORDER BY g.genre_key LIMIT 1)))"
        ));
}

impl Catalog {
    #[tracing::instrument(level = "debug", skip_all, err(Debug), fields(account_id = actor.id()))]
    pub async fn genres(&self, actor: &Actor) -> Result<Vec<Genre>> {
        self.genres_within(actor, GENRE_COUNT_CAP).await
    }

    /// Tiles whose counts stop at `cap`: each genre is read through its index until the cap, so
    /// the work depends on the number of genres and the cap, not on how many tracks a genre has.
    pub(crate) async fn genres_within(&self, actor: &Actor, cap: i64) -> Result<Vec<Genre>> {
        let mut query = QueryBuilder::new("WITH RECURSIVE keys(k, n) AS (SELECT ");
        push_next_key(&mut query, actor, "''");
        query.push(", 1 UNION ALL SELECT ");
        push_next_key(&mut query, actor, "keys.k");
        query.push(", n + 1 FROM keys WHERE k IS NOT NULL AND n < ");
        query.push_bind(GENRE_SCAN).push(
            ")
            SELECT k AS key, (SELECT label FROM (",
        );
        genre_recordings(
            &mut query,
            &self.host,
            actor,
            &Key::Column("keys.k"),
            "g.label AS label",
            1,
        );
        query.push(") LIMIT 1) AS label, (SELECT count(*) FROM (SELECT id FROM (");
        genre_recordings(
            &mut query,
            &self.host,
            actor,
            &Key::Column("keys.k"),
            "r.id AS id",
            cap,
        );
        query
            .push(") GROUP BY id LIMIT ")
            .push_bind(cap)
            .push(")) AS track_count FROM keys WHERE k IS NOT NULL AND label IS NOT NULL ORDER BY track_count DESC, k LIMIT ")
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
        let mut label = QueryBuilder::new("SELECT label FROM (");
        genre_recordings(
            &mut label,
            &self.host,
            actor,
            &Key::Bound(&key),
            "g.label AS label",
            1,
        );
        label.push(") LIMIT 1");
        let label: Option<String> = label.build_query_scalar().fetch_optional(&mut *tx).await?;
        let Some(label) = label else {
            return Err(Error::NotFound);
        };
        let mut count = QueryBuilder::new("SELECT count(*) FROM (SELECT id FROM (");
        genre_recordings(
            &mut count,
            &self.host,
            actor,
            &Key::Bound(&key),
            "r.id AS id",
            GENRE_COUNT_CAP,
        );
        count
            .push(") GROUP BY id LIMIT ")
            .push_bind(GENRE_COUNT_CAP)
            .push(")");
        let track_count: i64 = count.build_query_scalar().fetch_one(&mut *tx).await?;
        let genre = Genre {
            key: key.clone(),
            label,
            track_count,
        };
        let mut ids = QueryBuilder::new("SELECT id FROM (SELECT id, title FROM (");
        genre_recordings(
            &mut ids,
            &self.host,
            actor,
            &Key::Bound(&key),
            "r.id AS id, r.title AS title",
            GENRE_COUNT_CAP,
        );
        ids.push(") GROUP BY id LIMIT ")
            .push_bind(GENRE_COUNT_CAP)
            .push(") ORDER BY title COLLATE NOCASE, id LIMIT ")
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
        let mut albums = QueryBuilder::new("SELECT release_id FROM (SELECT id, release_id FROM (");
        genre_recordings(
            &mut albums,
            &self.host,
            actor,
            &Key::Bound(&key),
            "r.id AS id, r.release_id AS release_id",
            GENRE_COUNT_CAP,
        );
        albums
            .push(") GROUP BY id LIMIT ")
            .push_bind(GENRE_COUNT_CAP)
            .push(") WHERE release_id IS NOT NULL GROUP BY release_id ORDER BY count(*) DESC, release_id LIMIT ")
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
