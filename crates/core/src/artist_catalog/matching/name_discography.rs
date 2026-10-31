//! Matches credits to a MusicBrainz artist by name, but only when the artist's own recordings
//! corroborate the library: a name alone never matches. Corroboration asks about library titles
//! one search at a time instead of paging an artist's whole catalog.
use super::{
    ArtistProviderMatchReceipt,
    apply::{EvidenceKind, REPLACEABLE},
    name_probe::{
        self, Assessment, Candidate, MAX_NAMESAKES, MIN_CORROBORATING, PROBES_PER_SEARCH, Track,
        normalize,
    },
    provider_evidence::{LOOKUP_DEADLINE, LOOKUP_WINDOW, MAX_CONSECUTIVE_FAILURES, MAX_LOOKUPS},
    settle::{Checks, Examined, Outcome, Settlement},
};
use crate::{Actor, ArtistCatalog, Result};
use hito_metadata_musicbrainz::{ArtistRecordings, Error as MusicBrainzError};
use std::{collections::HashMap, future::Future, time::Duration};
use tokio::time::Instant;
use uuid::Uuid;

/// Cursor prefix of the name phase; the rest is the last credit identifier already handled.
pub(super) const NAME_CURSOR: &str = "name:";
/// A credit that is not finished in one request keeps the cursor on the credit before it, with a
/// hold count after this mark so that the cursor still moves forward.
const HOLD_MARK: char = '~';
const MAX_HOLDS: u32 = 40;
/// The rule that settled a name check. Checks of another rule do not hide a recording.
pub(super) const NAME_RULE: i64 = 2;
const MAX_CREDITS: i64 = 8;
const MAX_CREDIT_TRACKS: i64 = 200;
/// The least a credit costs: one search and one sample search per exact-name artist.
const MAX_CREDIT_LOOKUPS: usize = 1 + MAX_NAMESAKES;

struct Credit {
    id: String,
    name: String,
}

/// Why the tracks of a credit that were not asked about were left.
#[derive(Clone, Copy)]
enum Cause {
    /// The request ran out of lookups or time; the credit continues in the next request.
    Budget,
    /// MusicBrainz failed; the credit is asked again by a later walk.
    Outage,
}

/// The distinct library recordings of a credit and the sample that decides.
struct Plan {
    candidates: Vec<Candidate>,
    sample: Vec<usize>,
}

enum Verdict {
    Artist {
        artist: String,
        matched: Vec<usize>,
        unreached: Vec<usize>,
        cause: Option<Cause>,
    },
    Uncertain,
    Unavailable,
    Refused,
}

/// A fixed word for what went wrong with a lookup; never the request or its answer.
fn failure_reason(error: &MusicBrainzError) -> &'static str {
    match error {
        MusicBrainzError::Transport => "transport",
        MusicBrainzError::Status(429) => "status_429",
        MusicBrainzError::Status(503) => "status_503",
        MusicBrainzError::Status(500..=599) => "status_5xx",
        MusicBrainzError::Status(_) => "status_other",
        MusicBrainzError::ResponseTooLarge => "response_too_large",
        MusicBrainzError::InvalidLookup
        | MusicBrainzError::InvalidSearch
        | MusicBrainzError::InvalidResponse => "invalid_answer",
        _ => "request_rejected",
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

    /// Whether one more lookup fits in the request: the lookup cap as well as the window.
    fn has_room(&self) -> bool {
        self.lookups < MAX_LOOKUPS && self.allows()
    }

    /// Why `allows` said no, for the debug log.
    fn refusal(&self) -> &'static str {
        if self.failures >= MAX_CONSECUTIVE_FAILURES {
            "failure_limit"
        } else {
            "window_closed"
        }
    }

    async fn run<T>(
        &mut self,
        call: impl Future<Output = std::result::Result<T, MusicBrainzError>>,
    ) -> std::result::Result<T, MusicBrainzError> {
        let remaining = LOOKUP_DEADLINE.saturating_sub(self.started.elapsed());
        let answer = tokio::time::timeout(remaining, call)
            .await
            .unwrap_or_else(|_| {
                tracing::debug!(reason = "deadline", "Artist name lookup did not finish");
                Err(MusicBrainzError::Transport)
            });
        if let Err(error) = &answer {
            tracing::debug!(reason = failure_reason(error), "Artist name lookup failed");
        }
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
    recordings: HashMap<(String, String), ArtistRecordings>,
}

/// Where the cursor of a name request stands: the last credit handled and how often the credit
/// after it was held for lack of budget.
fn split_cursor(after: &str) -> (&str, u32) {
    match after.split_once(HOLD_MARK) {
        Some((credit, held)) => (credit, held.parse().unwrap_or(0)),
        None => (after, 0),
    }
}

fn held_cursor(credit: &str, held: u32) -> String {
    format!("{NAME_CURSOR}{credit}{HOLD_MARK}{held:02}")
}

impl ArtistCatalog {
    /// One bounded batch of the name phase: credits are examined in identifier order after the
    /// cursor, at most `MAX_LOOKUPS` MusicBrainz requests are made, and the pass stops early when
    /// the lookup window closes or lookups keep failing. A credit whose artist is found but whose
    /// other tracks do not all fit in the request is held: it is examined again next time.
    pub(super) async fn match_names(
        &self,
        actor: &Actor,
        request_id: Uuid,
        digest: &str,
        after: &str,
    ) -> Result<ArtistProviderMatchReceipt> {
        let (after_credit, held) = split_cursor(after);
        let (credits, more) = {
            let mut connection = self.store.pool.acquire().await?;
            actor.check_session_on(&mut connection).await?;
            credits(&mut connection, actor, after_credit).await?
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
        let mut holding = false;
        let mut budget = Budget::new(LOOKUP_WINDOW);
        let mut answers = Answers::default();
        {
            let _admission = self.admit_matching(actor)?;
            for (index, credit) in credits.iter().enumerate() {
                if index > 0 && budget.lookups + MAX_CREDIT_LOOKUPS > MAX_LOOKUPS {
                    break;
                }
                let verdict = self
                    .judge(credit, &tracks[index], &mut budget, &mut answers)
                    .await;
                let verdict = match verdict {
                    Verdict::Refused if index > 0 => {
                        tracing::debug!(
                            reason = budget.refusal(),
                            "Artist name credit waits for the next request"
                        );
                        break;
                    }
                    Verdict::Refused => {
                        tracing::debug!(
                            reason = budget.refusal(),
                            "Artist name credit unavailable"
                        );
                        Verdict::Unavailable
                    }
                    verdict => verdict,
                };
                let holds = matches!(
                    &verdict,
                    Verdict::Artist {
                        cause: Some(Cause::Budget),
                        ..
                    }
                ) && (index > 0 || held < MAX_HOLDS);
                for (position, track) in tracks[index].iter().enumerate() {
                    let outcome = match &verdict {
                        Verdict::Artist {
                            artist, matched, ..
                        } if matched.contains(&position) => {
                            Outcome::Artist(artist.clone(), EvidenceKind::NameDiscography)
                        }
                        Verdict::Artist {
                            unreached, cause, ..
                        } if unreached.contains(&position) => match cause {
                            Some(Cause::Outage) => Outcome::Unavailable,
                            _ => continue,
                        },
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
                if holds {
                    holding = true;
                    handled = index;
                    break;
                }
                handled = index + 1;
            }
        }
        let next_after = if holding {
            let previous = handled
                .checked_sub(1)
                .map_or(after_credit, |before| credits[before].id.as_str());
            let count = if handled == 0 { held + 1 } else { 1 };
            Some(held_cursor(previous, count))
        } else {
            (handled < credits.len() || more)
                .then(|| format!("{NAME_CURSOR}{}", credits[handled - 1].id))
        };
        tracing::debug!(
            credits = handled,
            held = holding,
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
        let candidates = name_probe::candidates(tracks);
        let sample = name_probe::sample(&candidates);
        if sample.len() < MIN_CORROBORATING {
            return Verdict::Uncertain;
        }
        let key = normalize(&credit.name);
        let found = match answers.namesakes.get(&key) {
            Some(found) => found.clone(),
            None => {
                if !budget.allows() {
                    return Verdict::Refused;
                }
                let found = match budget
                    .run(self.builtin.artist_name_search(&credit.name))
                    .await
                {
                    Ok(search) => name_probe::namesakes(&search, &credit.name),
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
        let mut assessments = Vec::new();
        for id in ids {
            let listed = match self.probe(&id, &candidates, &sample, budget, answers).await {
                Ok(listed) => listed,
                Err(verdict) => return verdict,
            };
            let assessment = name_probe::assess(&candidates, &sample, &listed);
            if assessment == Assessment::NotRead {
                return Verdict::Uncertain;
            }
            assessments.push((id, assessment));
        }
        match name_probe::decide(assessments) {
            Some((artist, hits)) => {
                let plan = Plan { candidates, sample };
                self.follow(artist, &plan, hits, budget, answers).await
            }
            None => Verdict::Uncertain,
        }
    }

    /// Asks one exact-name artist about the picked library recordings; answers already read in this
    /// request are reused.
    async fn probe(
        &self,
        artist: &str,
        candidates: &[Candidate],
        picked: &[usize],
        budget: &mut Budget,
        answers: &mut Answers,
    ) -> std::result::Result<ArtistRecordings, Verdict> {
        let probes = name_probe::probes(candidates, picked);
        let signature = format!("{probes:?}");
        let cache = (artist.to_owned(), signature);
        if let Some(known) = answers.recordings.get(&cache) {
            return Ok(known.clone());
        }
        if !budget.allows() {
            return Err(Verdict::Refused);
        }
        let listed = budget
            .run(self.builtin.artist_recordings_matching(artist, &probes))
            .await
            .map_err(|_| Verdict::Unavailable)?;
        answers.recordings.insert(cache, listed.clone());
        Ok(listed)
    }

    /// The tracks of the sample that the artist corroborates, plus the credit's other tracks whose
    /// own title and length the artist lists, asked about a few at a time while the request has
    /// room. Tracks not reached stay undecided and say why.
    async fn follow(
        &self,
        artist: String,
        plan: &Plan,
        hits: Vec<usize>,
        budget: &mut Budget,
        answers: &mut Answers,
    ) -> Verdict {
        let Plan { candidates, sample } = plan;
        let mut matched: Vec<usize> = hits
            .iter()
            .flat_map(|hit| candidates[*hit].tracks.iter().copied())
            .collect();
        let rest: Vec<usize> = (0..candidates.len())
            .filter(|index| !sample.contains(index))
            .collect();
        let mut cause = None;
        let mut reached = 0;
        for chunk in rest.chunks(PROBES_PER_SEARCH) {
            if !budget.has_room() {
                cause = Some(Cause::Budget);
                break;
            }
            match self
                .probe(&artist, candidates, chunk, budget, answers)
                .await
            {
                Ok(listed) => {
                    for hit in name_probe::found(candidates, chunk, &listed) {
                        matched.extend(candidates[hit].tracks.iter().copied());
                    }
                    reached += chunk.len();
                }
                Err(Verdict::Refused) => {
                    cause = Some(Cause::Budget);
                    break;
                }
                Err(_) => {
                    cause = Some(Cause::Outage);
                    break;
                }
            }
        }
        matched.sort_unstable();
        matched.dedup();
        let unreached = rest[reached..]
            .iter()
            .flat_map(|index| candidates[*index].tracks.iter().copied())
            .collect();
        Verdict::Artist {
            artist,
            matched,
            unreached,
            cause,
        }
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
                AND k.artist_operation_id IS p.artist_operation_id AND k.rule={NAME_RULE})
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
    use super::{Answers, Budget, Credit, Track, Verdict, held_cursor, split_cursor};
    use crate::test_support::{
        musicbrainz::{FakeMusicBrainz, Reply},
        support,
    };
    use std::time::Duration;

    const ID: &str = "11111111-1111-4111-8111-111111111111";

    fn credit() -> Credit {
        Credit {
            id: "credit".into(),
            name: "Nova Quartet".into(),
        }
    }

    fn tracks() -> Vec<Track> {
        [("Song", 100_000), ("Other Song", 120_000)]
            .into_iter()
            .map(|(title, length)| Track {
                recording: title.into(),
                title: title.into(),
                length_ms: Some(length),
                observation: "o".into(),
                operation: None,
            })
            .collect()
    }

    async fn catalog(fake: &FakeMusicBrainz) -> crate::ArtistCatalog {
        let (_directory, hito, ..) =
            support::library_with_musicbrainz("name-budget", Some(fake.endpoint.clone())).await;
        hito.artist_catalog
    }

    #[test]
    fn a_held_cursor_moves_forward_and_names_the_credit_before_the_held_one() {
        assert_eq!(split_cursor("credit-a"), ("credit-a", 0));
        assert_eq!(split_cursor("credit-a~03"), ("credit-a", 3));
        assert_eq!(split_cursor(""), ("", 0));
        let first = held_cursor("credit-a", 2);
        let second = held_cursor("credit-a", 3);
        assert_eq!(first, "name:credit-a~02");
        assert!(
            second > first,
            "The web walk continues only while the cursor advances"
        );
        assert!(held_cursor("credit-a", 10) > held_cursor("credit-a", 9));
        assert!(held_cursor("credit-a", 1).as_str() > "name:credit-a");
        assert!("name:credit-b" > held_cursor("credit-a", 40).as_str());
    }

    #[tokio::test]
    async fn the_window_is_checked_before_every_lookup_of_a_credit() {
        let fake = FakeMusicBrainz::start().await;
        fake.search(
            "Nova Quartet",
            Reply::Candidates(vec![(ID.into(), "Nova Quartet".into())]),
        );
        fake.recordings_of(ID, Reply::Discography(vec![]));
        fake.delay_replies(Duration::from_millis(600));
        let catalog = catalog(&fake).await;
        let mut budget = Budget::new(Duration::from_millis(500));
        let verdict = catalog
            .judge(&credit(), &tracks(), &mut budget, &mut Answers::default())
            .await;
        assert!(matches!(verdict, Verdict::Refused));
        assert_eq!(budget.refusal(), "window_closed");
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
        assert_eq!(budget.refusal(), "failure_limit");
        assert_eq!(fake.requests().len(), 3);
    }

    #[tokio::test]
    async fn a_credit_with_fewer_than_two_titles_of_known_length_sends_nothing() {
        let fake = FakeMusicBrainz::start().await;
        let catalog = catalog(&fake).await;
        let mut one = tracks();
        one.truncate(1);
        let verdict = catalog
            .judge(
                &credit(),
                &one,
                &mut Budget::new(Duration::from_secs(9)),
                &mut Answers::default(),
            )
            .await;
        assert!(matches!(verdict, Verdict::Uncertain));
        assert!(fake.requests().is_empty());
    }
}
