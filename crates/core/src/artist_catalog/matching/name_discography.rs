//! Matches credits to a MusicBrainz artist by name, but only when the artist's own recordings
//! corroborate the library: a name alone never matches.
use super::{
    ArtistProviderMatchReceipt,
    apply::{EvidenceKind, REPLACEABLE},
    provider_evidence::{
        LOOKUP_DEADLINE, LOOKUP_WINDOW, MAX_CONSECUTIVE_FAILURES, MAX_LOOKUPS,
        SPECIAL_PURPOSE_ARTISTS,
    },
    settle::{Checks, Examined, Outcome, Settlement},
};
use crate::{Actor, ArtistCatalog, Result};
use hito_metadata_musicbrainz::{ArtistRecordings, Error as MusicBrainzError};
use hito_plugin_sdk::{ArtistSearchPage, ArtistSearchRequest};
use std::{collections::HashMap, future::Future, time::Duration};
use tokio::time::Instant;
use uuid::Uuid;

/// Cursor prefix of the name phase; the rest is the last credit identifier already handled.
pub(super) const NAME_CURSOR: &str = "name:";
const MAX_CREDITS: i64 = 8;
const MAX_CREDIT_TRACKS: i64 = 200;
const SEARCH_LIMIT: u16 = 10;
/// More exact-name artists than this cannot be told apart within one request.
const MAX_NAMESAKES: usize = 3;
const MAX_CREDIT_LOOKUPS: usize = 1 + MAX_NAMESAKES;
const LENGTH_TOLERANCE_MS: u64 = 2000;
/// Distinct MusicBrainz recordings a library must share with an artist to corroborate it.
const MIN_CORROBORATING: usize = 2;

struct Credit {
    id: String,
    name: String,
}

struct Track {
    recording: String,
    title: String,
    length_ms: Option<u64>,
    observation: String,
    operation: Option<String>,
}

enum Verdict {
    Artist(String, Vec<usize>),
    Uncertain,
    Unavailable,
    Refused,
}

/// Case-folded, whitespace-collapsed text. Punctuation and diacritics are significant.
fn normalize(text: &str) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

/// The ordinary artists whose name equals the credit, or `None` when the search cannot prove it
/// saw every one of them: too many namesakes, or a truncated page that still ends on an exact
/// name.
fn namesakes(page: &ArtistSearchPage, credit: &str) -> Option<Vec<String>> {
    let wanted = normalize(credit);
    let exact = |name: &str| normalize(name) == wanted;
    if page.next_cursor.is_some()
        && page
            .items
            .last()
            .is_some_and(|last| exact(&last.artist.name))
    {
        return None;
    }
    let found: Vec<String> = page
        .items
        .iter()
        .filter(|item| exact(&item.artist.name))
        .map(|item| item.artist.id.clone())
        .filter(|id| !SPECIAL_PURPOSE_ARTISTS.contains(&id.as_str()))
        .collect();
    (found.len() <= MAX_NAMESAKES).then_some(found)
}

/// Library tracks that equal a listed recording by normalized title and by length within the
/// tolerance, with how many distinct MusicBrainz recordings they matched.
fn corroborated(tracks: &[Track], listed: &ArtistRecordings) -> (Vec<usize>, usize) {
    let mut indices = Vec::new();
    let mut recordings: Vec<&str> = Vec::new();
    for (index, track) in tracks.iter().enumerate() {
        let Some(length) = track.length_ms else {
            continue;
        };
        let title = normalize(&track.title);
        let hit = listed.items.iter().find(|item| {
            normalize(&item.title) == title
                && item
                    .length_ms
                    .is_some_and(|other| other.abs_diff(length) <= LENGTH_TOLERANCE_MS)
        });
        if let Some(hit) = hit {
            indices.push(index);
            if !recordings.contains(&hit.id.as_str()) {
                recordings.push(&hit.id);
            }
        }
    }
    (indices, recordings.len())
}

/// Accepts an artist only when exactly one namesake corroborates the library, and no namesake
/// that does not could still do so with recordings that were not read.
fn decide(tracks: &[Track], namesakes: &[(String, ArtistRecordings)]) -> Verdict {
    let mut accepted: Option<(String, Vec<usize>)> = None;
    for (id, listed) in namesakes {
        let (indices, distinct) = corroborated(tracks, listed);
        if distinct >= MIN_CORROBORATING {
            if accepted.is_some() {
                return Verdict::Uncertain;
            }
            accepted = Some((id.clone(), indices));
        } else if !listed.complete() {
            return Verdict::Uncertain;
        }
    }
    match accepted {
        Some((id, indices)) => Verdict::Artist(id, indices),
        None => Verdict::Uncertain,
    }
}

struct Budget {
    started: Instant,
    window: Duration,
    lookups: usize,
    failures: usize,
}

impl Budget {
    fn new(window: Duration) -> Self {
        Self {
            started: Instant::now(),
            window,
            lookups: 0,
            failures: 0,
        }
    }

    /// The first lookup of a request is always allowed; later ones stop at the failure limit and
    /// when the window has closed. Checked before every lookup.
    fn allows(&self) -> bool {
        self.lookups == 0
            || (self.failures < MAX_CONSECUTIVE_FAILURES && self.started.elapsed() <= self.window)
    }

    async fn run<T>(
        &mut self,
        call: impl Future<Output = std::result::Result<T, MusicBrainzError>>,
    ) -> std::result::Result<T, MusicBrainzError> {
        let remaining = LOOKUP_DEADLINE.saturating_sub(self.started.elapsed());
        let answer = tokio::time::timeout(remaining, call)
            .await
            .unwrap_or(Err(MusicBrainzError::Transport));
        self.lookups += 1;
        self.failures = match &answer {
            Ok(_) | Err(MusicBrainzError::Status(404) | MusicBrainzError::InvalidSearchRequest) => {
                0
            }
            Err(_) => self.failures + 1,
        };
        answer
    }
}

#[derive(Default)]
struct Answers {
    namesakes: HashMap<String, Option<Vec<String>>>,
    recordings: HashMap<String, ArtistRecordings>,
}

impl ArtistCatalog {
    /// One bounded batch of the name phase: credits are examined in identifier order after the
    /// cursor, at most `MAX_LOOKUPS` MusicBrainz requests are made, and the pass stops early when
    /// the lookup window closes or lookups keep failing.
    pub(super) async fn match_names(
        &self,
        actor: &Actor,
        request_id: Uuid,
        digest: &str,
        after: &str,
    ) -> Result<ArtistProviderMatchReceipt> {
        let (mut credits, more) = {
            let mut connection = self.store.pool.acquire().await?;
            actor.check_session_on(&mut connection).await?;
            credits(&mut connection, actor, after).await?
        };
        let mut tracks = Vec::new();
        {
            let mut connection = self.store.pool.acquire().await?;
            for credit in &credits {
                tracks.push(credit_tracks(&mut connection, actor, &credit.id).await?);
            }
        }
        let mut examined = Vec::new();
        let mut handled = 0;
        let mut budget = Budget::new(LOOKUP_WINDOW);
        let mut answers = Answers::default();
        {
            let _admission = self.admit(actor.id())?;
            for (index, credit) in credits.iter().enumerate() {
                if index > 0 && budget.lookups + MAX_CREDIT_LOOKUPS > MAX_LOOKUPS {
                    break;
                }
                let verdict = self
                    .judge(credit, &tracks[index], &mut budget, &mut answers)
                    .await;
                let verdict = match verdict {
                    Verdict::Refused if index > 0 => break,
                    Verdict::Refused => Verdict::Unavailable,
                    verdict => verdict,
                };
                for (position, track) in tracks[index].iter().enumerate() {
                    let outcome = match &verdict {
                        Verdict::Artist(artist, matched) if matched.contains(&position) => {
                            Outcome::Artist(artist.clone(), EvidenceKind::NameDiscography)
                        }
                        Verdict::Unavailable => Outcome::Unavailable,
                        _ => Outcome::Uncertain,
                    };
                    examined.push(Examined {
                        recording: track.recording.clone(),
                        observation: track.observation.clone(),
                        operation: track.operation.clone(),
                        outcome,
                    });
                }
                handled = index + 1;
            }
        }
        let next_after = (handled < credits.len() || more)
            .then(|| format!("{NAME_CURSOR}{}", credits[handled - 1].id));
        credits.truncate(handled);
        tracing::debug!(
            credits = handled,
            lookups = budget.lookups,
            recordings = examined.len(),
            elapsed_ms = budget.started.elapsed().as_millis() as u64,
            "Artist name matching examined credits"
        );
        self.settle(
            actor,
            Settlement {
                request_id,
                digest,
                next_after,
                checks: Checks::Name,
            },
            examined,
        )
        .await
    }

    async fn judge(
        &self,
        credit: &Credit,
        tracks: &[Track],
        budget: &mut Budget,
        answers: &mut Answers,
    ) -> Verdict {
        let key = normalize(&credit.name);
        let found = match answers.namesakes.get(&key) {
            Some(found) => found.clone(),
            None => {
                if !budget.allows() {
                    return Verdict::Refused;
                }
                let request = ArtistSearchRequest {
                    query: credit.name.clone(),
                    cursor: None,
                    snapshot: None,
                    limit: SEARCH_LIMIT,
                };
                let found = match budget.run(self.builtin.artist_search(request)).await {
                    Ok(page) => namesakes(&page, &credit.name),
                    Err(MusicBrainzError::Status(404) | MusicBrainzError::InvalidSearchRequest) => {
                        None
                    }
                    Err(_) => return Verdict::Unavailable,
                };
                answers.namesakes.insert(key, found.clone());
                found
            }
        };
        let Some(ids) = found.filter(|ids| !ids.is_empty()) else {
            return Verdict::Uncertain;
        };
        let mut listed = Vec::new();
        for id in ids {
            let recordings = match answers.recordings.get(&id) {
                Some(recordings) => recordings.clone(),
                None => {
                    if !budget.allows() {
                        return Verdict::Refused;
                    }
                    match budget.run(self.builtin.artist_recordings(&id)).await {
                        Ok(recordings) => {
                            answers.recordings.insert(id.clone(), recordings.clone());
                            recordings
                        }
                        Err(_) => return Verdict::Unavailable,
                    }
                }
            };
            listed.push((id, recordings));
        }
        decide(tracks, &listed)
    }
}

/// Library recordings of a credit the account has not decided about and no earlier name pass
/// settled for the same observation and personal artist operation.
fn eligible(user: &str, artist: &str) -> String {
    format!(
        "SELECT r.id,r.title,r.duration_ms,r.metadata_observation_id,p.artist_operation_id
         FROM account_recording_credits rc
         JOIN recordings r ON r.id=rc.recording_id
         JOIN library_members m ON m.library_id=r.library_id AND m.user_id=rc.user_id
         LEFT JOIN personal_recording_metadata p ON p.recording_id=r.id AND p.user_id=rc.user_id
         WHERE rc.user_id={user} AND rc.artist_id={artist} AND r.metadata_observation_id IS NOT NULL
            AND NOT EXISTS (SELECT 1 FROM artist_name_checks k WHERE k.user_id=rc.user_id
                AND k.recording_id=r.id AND k.observation_id=r.metadata_observation_id
                AND k.artist_operation_id IS p.artist_operation_id)
            AND NOT EXISTS (SELECT 1 FROM recording_artist_identity_choices c
                WHERE c.user_id=rc.user_id AND c.recording_id=r.id AND NOT {REPLACEABLE})"
    )
}

/// Credits with at least two undecided recordings, since fewer can never corroborate.
async fn credits(
    connection: &mut sqlx::SqliteConnection,
    actor: &Actor,
    after: &str,
) -> Result<(Vec<Credit>, bool)> {
    let mut rows: Vec<(String, String)> = sqlx::query_as(&format!(
        "SELECT a.id,a.name FROM account_artists a
         JOIN library_members m ON m.library_id=a.library_id AND m.user_id=a.user_id
         WHERE a.user_id=? AND a.id>? AND (SELECT count(*) FROM ({} LIMIT 2))>=2
         ORDER BY a.id LIMIT ?",
        eligible("a.user_id", "a.id")
    ))
    .bind(actor.id())
    .bind(after)
    .bind(MAX_CREDITS + 1)
    .fetch_all(connection)
    .await?;
    let more = rows.len() as i64 > MAX_CREDITS;
    rows.truncate(MAX_CREDITS as usize);
    let credits = rows
        .into_iter()
        .map(|(id, name)| Credit { id, name })
        .collect();
    Ok((credits, more))
}

#[derive(sqlx::FromRow)]
struct TrackRow {
    id: String,
    title: String,
    duration_ms: Option<i64>,
    metadata_observation_id: String,
    artist_operation_id: Option<String>,
}

async fn credit_tracks(
    connection: &mut sqlx::SqliteConnection,
    actor: &Actor,
    artist: &str,
) -> Result<Vec<Track>> {
    let rows: Vec<TrackRow> =
        sqlx::query_as(&format!("{} ORDER BY r.id LIMIT ?", eligible("?", "?")))
            .bind(actor.id())
            .bind(artist)
            .bind(MAX_CREDIT_TRACKS)
            .fetch_all(connection)
            .await?;
    Ok(rows
        .into_iter()
        .map(|row| Track {
            recording: row.id,
            title: row.title,
            length_ms: row
                .duration_ms
                .and_then(|length| u64::try_from(length).ok()),
            observation: row.metadata_observation_id,
            operation: row.artist_operation_id,
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::{Answers, Budget, Credit, Track, Verdict, namesakes, normalize};
    use crate::test_support::{
        musicbrainz::{FakeMusicBrainz, Reply},
        support,
    };
    use hito_plugin_sdk::{
        ArtistSearchCandidate, ArtistSearchPage, CatalogArtistKind, CatalogArtistRef,
    };
    use std::time::Duration;

    const ID: &str = "11111111-1111-4111-8111-111111111111";
    const OTHER: &str = "22222222-2222-4222-8222-222222222222";
    const VARIOUS_ARTISTS: &str = "89ad4ac3-39f7-470e-963a-56509c546377";

    fn page(names: &[(&str, &str)], truncated: bool) -> ArtistSearchPage {
        ArtistSearchPage {
            query: "q".into(),
            snapshot: None,
            items: names
                .iter()
                .map(|(id, name)| ArtistSearchCandidate {
                    artist: CatalogArtistRef {
                        id: (*id).into(),
                        name: (*name).into(),
                    },
                    kind: Some(CatalogArtistKind::Person),
                    disambiguation: None,
                })
                .collect(),
            total_count: None,
            next_cursor: truncated.then(|| "more".into()),
        }
    }

    #[test]
    fn names_compare_by_case_and_spacing_but_not_by_punctuation_or_accents() {
        assert_eq!(normalize("  Nova   QUARTET "), "nova quartet");
        assert_ne!(normalize("Nova Quartet"), normalize("Nova-Quartet"));
        assert_ne!(normalize("Beyoncé"), normalize("Beyonce"));
    }

    #[test]
    fn only_a_search_that_proves_it_saw_every_namesake_names_any() {
        let exact = [(ID, "Nova Quartet"), (OTHER, "NOVA quartet")];
        assert_eq!(
            namesakes(&page(&exact, false), "Nova Quartet"),
            Some(vec![ID.into(), OTHER.into()])
        );
        let tail = [(ID, "Nova Quartet"), (OTHER, "Nova Quartet Tribute")];
        assert_eq!(
            namesakes(&page(&tail, true), "Nova Quartet"),
            Some(vec![ID.into()]),
            "A truncated page that ends on another name has shown every exact name"
        );
        assert_eq!(namesakes(&page(&exact, true), "Nova Quartet"), None);
        let many = [
            (ID, "A"),
            (OTHER, "A"),
            ("33333333-3333-4333-8333-333333333333", "A"),
            ("44444444-4444-4444-8444-444444444444", "A"),
        ];
        assert_eq!(namesakes(&page(&many, false), "A"), None);
        assert_eq!(
            namesakes(
                &page(&[(VARIOUS_ARTISTS, "Various Artists")], false),
                "Various Artists"
            ),
            Some(vec![])
        );
    }

    fn credit() -> Credit {
        Credit {
            id: "credit".into(),
            name: "Nova Quartet".into(),
        }
    }

    fn tracks() -> Vec<Track> {
        vec![Track {
            recording: "r".into(),
            title: "Song".into(),
            length_ms: Some(1000),
            observation: "o".into(),
            operation: None,
        }]
    }

    async fn catalog(fake: &FakeMusicBrainz) -> crate::ArtistCatalog {
        let (_directory, hito, ..) =
            support::library_with_musicbrainz("name-budget", Some(fake.endpoint.clone())).await;
        hito.artist_catalog
    }

    #[tokio::test]
    async fn the_window_is_checked_before_every_lookup_of_a_credit() {
        let fake = FakeMusicBrainz::start().await;
        fake.search(
            "Nova Quartet",
            Reply::Candidates(vec![(ID.into(), "Nova Quartet".into())]),
        );
        fake.recordings_of(ID, Reply::Discography(0, vec![]));
        fake.delay_replies(Duration::from_millis(600));
        let catalog = catalog(&fake).await;
        let mut budget = Budget::new(Duration::from_millis(500));
        let verdict = catalog
            .judge(&credit(), &tracks(), &mut budget, &mut Answers::default())
            .await;
        assert!(matches!(verdict, Verdict::Refused));
        assert_eq!(fake.requests().len(), 1, "{:?}", fake.requests());
    }

    #[tokio::test]
    async fn three_consecutive_failures_refuse_every_later_lookup() {
        let fake = FakeMusicBrainz::start().await;
        fake.search("Nova Quartet", Reply::Unavailable);
        let catalog = catalog(&fake).await;
        let mut budget = Budget::new(Duration::from_secs(9));
        let mut answers = Answers::default();
        for _ in 0..3 {
            answers.namesakes.clear();
            assert!(matches!(
                catalog
                    .judge(&credit(), &tracks(), &mut budget, &mut answers)
                    .await,
                Verdict::Unavailable
            ));
        }
        answers.namesakes.clear();
        assert!(matches!(
            catalog
                .judge(&credit(), &tracks(), &mut budget, &mut answers)
                .await,
            Verdict::Refused
        ));
        assert_eq!(fake.requests().len(), 3);
    }
}
