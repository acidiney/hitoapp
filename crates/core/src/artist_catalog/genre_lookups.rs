use super::{ArtistCatalog, MetadataProvider, matching::*, policy};
use crate::{Actor, Result, catalog::genre_identity};
use hito_metadata_musicbrainz::GenreEntity;
use sqlx::SqliteConnection;
use std::time::Instant;
use tokio::time::Instant as Clock;
use uuid::Uuid;

/// Marks the genre pass inside the stored position of an automatic walk.
pub(crate) const GENRE_CURSOR: &str = "genres:";
const REFRESH_SECONDS: i64 = 30 * 24 * 60 * 60;
const MAX_GENRES_PER_ENTITY: usize = 8;

const ENTITIES: [(char, &str, GenreEntity); 3] = [
    ('1', "artist", GenreEntity::Artist),
    ('2', "release_group", GenreEntity::ReleaseGroup),
    ('3', "recording", GenreEntity::Recording),
];

pub(crate) struct GenreReceipt {
    pub next_after: Option<String>,
    pub looked_up: usize,
    pub unavailable: usize,
}

struct Candidate {
    rank: char,
    mbid: String,
}

impl Candidate {
    fn key(&self) -> String {
        format!("{}{}", self.rank, self.mbid)
    }

    fn entity(&self) -> (&'static str, GenreEntity) {
        ENTITIES
            .iter()
            .find(|(rank, ..)| *rank == self.rank)
            .map(|(_, name, entity)| (*name, *entity))
            .unwrap_or(("artist", GenreEntity::Artist))
    }
}

enum Answer {
    Genres(Vec<String>),
    Unavailable,
}

const NO_FILE_GENRE: &str = "NOT EXISTS (SELECT 1 FROM sources fs JOIN source_genres fg ON fg.source_id = fs.id WHERE fs.recording_id = r.id)";

/// The MusicBrainz identities the account's readable library names that have no fresh lookup,
/// artists first, then release groups, then recordings, in identifier order after `after`.
async fn pending(
    connection: &mut SqliteConnection,
    account: &str,
    after: Option<&str>,
    limit: i64,
) -> Result<Vec<Candidate>> {
    let (after_rank, after_mbid) = after
        .and_then(|after| {
            after
                .chars()
                .next()
                .map(|rank| (rank, &after[rank.len_utf8()..]))
        })
        .unwrap_or(('0', ""));
    let special = SPECIAL_PURPOSE_ARTISTS
        .map(|id| format!("'{id}'"))
        .join(",");
    let mut branches = Vec::new();
    for (rank, name, _) in ENTITIES {
        if rank < after_rank {
            continue;
        }
        let from = if rank == '1' {
            format!(
                "SELECT g.provider_artist_id AS mbid FROM account_artist_identity_groups g
                 WHERE g.user_id = ?1 AND g.provider_kind = 'builtin_musicbrainz'
                    AND length(g.provider_artist_id) = 36 AND g.provider_artist_id NOT IN ({special})
                    {{after}} AND EXISTS (SELECT 1 FROM recording_artist_identity_choices c
                        JOIN recordings r ON r.id = c.recording_id
                        JOIN library_members m ON m.library_id = r.library_id AND m.user_id = ?1
                        WHERE c.user_id = ?1 AND c.group_id = g.id AND c.retired = 0
                            AND c.association_id IS NOT NULL)"
            )
        } else {
            format!(
                "SELECT DISTINCT o.mbid AS mbid FROM recording_musicbrainz_observations o
                 JOIN recordings r ON r.id = o.recording_id AND r.metadata_observation_id = o.observation_id
                 JOIN library_members m ON m.library_id = r.library_id AND m.user_id = ?1
                 WHERE o.entity = '{name}' {{after}} AND {NO_FILE_GENRE}"
            )
        };
        let column = if rank == '1' {
            "g.provider_artist_id"
        } else {
            "o.mbid"
        };
        let bound = if rank == after_rank && !after_mbid.is_empty() {
            format!("AND {column} > ?2")
        } else {
            String::new()
        };
        branches.push(format!(
            "SELECT * FROM (SELECT '{rank}' AS rank, mbid FROM ({}) AS found WHERE NOT EXISTS (
                SELECT 1 FROM musicbrainz_genre_lookups l WHERE l.entity = '{name}' AND l.mbid = found.mbid
                    AND l.fetched_at > unixepoch() - {REFRESH_SECONDS}) ORDER BY mbid LIMIT ?3)",
            from.replace("{after}", &bound)
        ));
    }
    if branches.is_empty() {
        return Ok(Vec::new());
    }
    let rows: Vec<(String, String)> = sqlx::query_as(&format!(
        "SELECT rank, mbid FROM ({}) ORDER BY rank, mbid LIMIT ?3",
        branches.join(" UNION ALL ")
    ))
    .bind(account)
    .bind(after_mbid)
    .bind(limit)
    .fetch_all(connection)
    .await?;
    Ok(rows
        .into_iter()
        .filter_map(|(rank, mbid)| {
            Some(Candidate {
                rank: rank.chars().next()?,
                mbid,
            })
        })
        .collect())
}

/// Whether the account's library names a MusicBrainz identity whose genres were never read or
/// are older than the refresh period.
pub(crate) async fn has_pending(connection: &mut SqliteConnection, account: &str) -> Result<bool> {
    Ok(!pending(connection, account, None, 1).await?.is_empty())
}

impl ArtistCatalog {
    /// Reads the official MusicBrainz genres of the identities the account's library names: its
    /// chosen artists, then the release groups and recordings named by file tags, a bounded
    /// batch per call. Answers are cached by identifier and asked again after thirty days. Only
    /// identifiers leave the host; an outage leaves the identity uncached for a later pass.
    #[tracing::instrument(level = "info", skip_all, fields(operation = "musicbrainz_genres", request_id = %Uuid::new_v4()))]
    pub(crate) async fn collect_musicbrainz_genres(
        &self,
        actor: &Actor,
        after: Option<&str>,
    ) -> Result<GenreReceipt> {
        let started = Instant::now();
        let result = self.read_genres(actor, after).await;
        match &result {
            Ok(receipt) => tracing::info!(
                outcome = "accepted",
                looked_up = receipt.looked_up,
                unavailable = receipt.unavailable,
                complete = receipt.next_after.is_none(),
                elapsed_ms = started.elapsed().as_millis() as u64,
                "MusicBrainz genre lookups finished"
            ),
            Err(error) => tracing::info!(
                outcome = error.code(),
                elapsed_ms = started.elapsed().as_millis() as u64,
                "MusicBrainz genre lookups finished"
            ),
        }
        result
    }

    async fn read_genres(&self, actor: &Actor, after: Option<&str>) -> Result<GenreReceipt> {
        let candidates = {
            let mut connection = self.store.pool.acquire().await?;
            actor.check_session_on(&mut connection).await?;
            let settings = policy::read(&mut connection, actor).await?;
            if !settings.automatic_artist_lookup
                || !policy::selected(&settings, &MetadataProvider::BuiltinMusicBrainz)
            {
                return Ok(GenreReceipt {
                    next_after: None,
                    looked_up: 0,
                    unavailable: 0,
                });
            }
            pending(&mut connection, actor.id(), after, MAX_LOOKUPS as i64 + 1).await?
        };
        let more = candidates.len() > MAX_LOOKUPS;
        let candidates = &candidates[..candidates.len().min(MAX_LOOKUPS)];
        let (answers, examined) = {
            let _admission = self.admit_matching(actor)?;
            self.ask_genres(candidates).await
        };
        let mut tx = self.store.pool.begin_with("BEGIN IMMEDIATE").await?;
        actor.check_session_on(&mut tx).await?;
        let (mut looked_up, mut unavailable) = (0, 0);
        for (candidate, answer) in candidates.iter().zip(&answers) {
            match answer {
                Answer::Genres(names) => {
                    store(&mut tx, candidate, names).await?;
                    looked_up += 1;
                }
                Answer::Unavailable => unavailable += 1,
            }
        }
        tx.commit().await?;
        let next_after = if examined < candidates.len() || more {
            candidates[..examined].last().map(Candidate::key)
        } else {
            None
        };
        Ok(GenreReceipt {
            next_after,
            looked_up,
            unavailable,
        })
    }

    /// Asks for each candidate in turn within the lookup count and time bounds; the first is
    /// always asked. Returns the answers and how many leading candidates were examined.
    async fn ask_genres(&self, candidates: &[Candidate]) -> (Vec<Answer>, usize) {
        let started = Clock::now();
        let mut answers = Vec::new();
        let mut failures = 0;
        for (index, candidate) in candidates.iter().enumerate() {
            if index > 0
                && (failures >= MAX_CONSECUTIVE_FAILURES || started.elapsed() > LOOKUP_WINDOW)
            {
                break;
            }
            let remaining = LOOKUP_DEADLINE.saturating_sub(started.elapsed());
            let (_, entity) = candidate.entity();
            let asked =
                tokio::time::timeout(remaining, self.builtin.genres(entity, &candidate.mbid)).await;
            let answer = match asked {
                Ok(Ok(names)) => Answer::Genres(names),
                Ok(Err(hito_metadata_musicbrainz::Error::InvalidLookupRequest)) => {
                    Answer::Genres(Vec::new())
                }
                _ => Answer::Unavailable,
            };
            failures = if matches!(answer, Answer::Unavailable) {
                failures + 1
            } else {
                0
            };
            answers.push(answer);
        }
        let examined = answers.len();
        (answers, examined)
    }
}

async fn store(
    connection: &mut SqliteConnection,
    candidate: &Candidate,
    names: &[String],
) -> Result<()> {
    let (entity, _) = candidate.entity();
    sqlx::query(
        "INSERT INTO musicbrainz_genre_lookups(entity, mbid, fetched_at) VALUES (?, ?, unixepoch())
         ON CONFLICT(entity, mbid) DO UPDATE SET fetched_at = excluded.fetched_at",
    )
    .bind(entity)
    .bind(&candidate.mbid)
    .execute(&mut *connection)
    .await?;
    sqlx::query("DELETE FROM musicbrainz_genres WHERE entity = ? AND mbid = ?")
        .bind(entity)
        .bind(&candidate.mbid)
        .execute(&mut *connection)
        .await?;
    let mut kept: Vec<(String, String)> = Vec::new();
    for name in names {
        let Some((key, label)) = genre_identity(name) else {
            continue;
        };
        if kept.len() < MAX_GENRES_PER_ENTITY && !kept.iter().any(|(known, _)| *known == key) {
            kept.push((key, label));
        }
    }
    for (key, label) in kept {
        sqlx::query(
            "INSERT INTO musicbrainz_genres(entity, mbid, genre_key, label) VALUES (?, ?, ?, ?)",
        )
        .bind(entity)
        .bind(&candidate.mbid)
        .bind(key)
        .bind(label)
        .execute(&mut *connection)
        .await?;
    }
    Ok(())
}
