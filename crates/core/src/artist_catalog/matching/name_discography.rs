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
use hito_metadata_musicbrainz::{ArtistRecordings, Error as MusicBrainzError, NameSearch};
use std::{collections::HashMap, future::Future, time::Duration};
use tokio::time::Instant;
use uuid::Uuid;

/// Cursor prefix of the name phase; the rest is the last credit identifier already handled.
pub(super) const NAME_CURSOR: &str = "name:";
const MAX_CREDITS: i64 = 8;
const MAX_CREDIT_TRACKS: i64 = 200;
/// More exact-name artists than this cannot be told apart within one request.
const MAX_NAMESAKES: usize = 3;
/// Recordings pages one credit may read in total, and at most this many for one artist.
const MAX_CREDIT_PAGES: usize = 6;
const MAX_ARTIST_PAGES: usize = 5;
const PAGE_SIZE: usize = 100;
const MAX_CREDIT_LOOKUPS: usize = 1 + MAX_CREDIT_PAGES;
const LENGTH_TOLERANCE_MS: u64 = 2000;
/// A shorter length is a placeholder, not a measurement.
const MIN_KNOWN_LENGTH_MS: u64 = 1000;
/// Distinct normalized titles a library must share with an artist to corroborate it.
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
/// saw every one of them: too many namesakes, or a page that may end before the last
/// maximum-relevance result (an exact name always scores the maximum).
fn namesakes(search: &NameSearch, credit: &str) -> Option<Vec<String>> {
    let wanted = normalize(credit);
    let scoring_holds = search
        .items
        .iter()
        .filter(|item| normalize(&item.name) == wanted)
        .all(|item| item.score >= 100);
    if !(search.conclusive() && scoring_holds || search.items.len() as u32 >= search.total) {
        return None;
    }
    let found: Vec<String> = search
        .items
        .iter()
        .filter(|item| normalize(&item.name) == wanted)
        .map(|item| item.id.clone())
        .filter(|id| !SPECIAL_PURPOSE_ARTISTS.contains(&id.as_str()))
        .collect();
    (found.len() <= MAX_NAMESAKES).then_some(found)
}

/// Library tracks that equal a listed recording by normalized title and by length within the
/// tolerance, with how many distinct normalized titles they matched.
fn corroborated(tracks: &[Track], listed: &ArtistRecordings) -> (Vec<usize>, usize) {
    let mut indices = Vec::new();
    let mut titles: Vec<String> = Vec::new();
    for (index, track) in tracks.iter().enumerate() {
        let Some(length) = track.length_ms else {
            continue;
        };
        let title = normalize(&track.title);
        let hit = listed.items.iter().find(|item| {
            normalize(&item.title) == title
                && item
                    .length_ms
                    .and_then(|other| known_length(i64::try_from(other).ok()))
                    .is_some_and(|other| other.abs_diff(length) <= LENGTH_TOLERANCE_MS)
        });
        if hit.is_some() {
            indices.push(index);
            if !titles.contains(&title) {
                titles.push(title);
            }
        }
    }
    (indices, titles.len())
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
        Some((id, indices)) => {
            let listed = namesakes.iter().find(|(known, _)| *known == id);
            let indices = listed
                .map(|(_, listed)| follow(tracks, indices.clone(), listed, namesakes.len() == 1))
                .unwrap_or(indices);
            Verdict::Artist(id, indices)
        }
        None => Verdict::Uncertain,
    }
}

/// The corroborated tracks plus the credit's other tracks whose own title the artist's recordings
/// list (lengths compared when both are known), or all of them when the artist is the only
/// namesake, its recordings were read completely and they corroborate at least half the credit.
fn follow(
    tracks: &[Track],
    mut indices: Vec<usize>,
    listed: &ArtistRecordings,
    sole: bool,
) -> Vec<usize> {
    if sole && listed.complete() && indices.len() * 2 >= tracks.len() {
        return (0..tracks.len()).collect();
    }
    for (index, track) in tracks.iter().enumerate() {
        if indices.contains(&index) {
            continue;
        }
        let title = normalize(&track.title);
        let library_length = track
            .length_ms
            .and_then(|length| known_length(i64::try_from(length).ok()));
        let found = listed.items.iter().any(|item| {
            normalize(&item.title) == title
                && match (
                    library_length,
                    item.length_ms
                        .and_then(|other| known_length(i64::try_from(other).ok())),
                ) {
                    (Some(ours), Some(theirs)) => ours.abs_diff(theirs) <= LENGTH_TOLERANCE_MS,
                    _ => true,
                }
        });
        if found {
            indices.push(index);
        }
    }
    indices.sort_unstable();
    indices
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
            let _admission = self.admit_matching(actor)?;
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
                let found = match budget
                    .run(self.builtin.artist_name_search(&credit.name))
                    .await
                {
                    Ok(search) => namesakes(&search, &credit.name),
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
        let pages = (MAX_CREDIT_PAGES / ids.len()).clamp(1, MAX_ARTIST_PAGES);
        for id in ids {
            let read = match self
                .read_recordings(&id, pages, tracks, budget, answers)
                .await
            {
                Ok(read) => read,
                Err(verdict) => return verdict,
            };
            listed.push((id, read));
        }
        decide(tracks, &listed)
    }
}

impl ArtistCatalog {
    /// Reads an artist's recordings page by page until the library corroborates it, the list is
    /// complete, or `pages` pages are read. Pages already read in this request are reused.
    async fn read_recordings(
        &self,
        id: &str,
        pages: usize,
        tracks: &[Track],
        budget: &mut Budget,
        answers: &mut Answers,
    ) -> std::result::Result<ArtistRecordings, Verdict> {
        let mut read = answers.recordings.get(id).cloned();
        loop {
            let offset = match &read {
                None => 0,
                Some(read)
                    if read.complete()
                        || read.items.len().div_ceil(PAGE_SIZE) >= pages
                        || corroborated(tracks, read).1 >= MIN_CORROBORATING =>
                {
                    break;
                }
                Some(read) => read.items.len(),
            };
            if !budget.allows() {
                return Err(Verdict::Refused);
            }
            let page = budget
                .run(self.builtin.artist_recordings_at(id, offset as u32))
                .await
                .map_err(|_| Verdict::Unavailable)?;
            match &mut read {
                None => read = Some(page),
                Some(known) => {
                    if !known.extend(page) {
                        break;
                    }
                }
            }
            if let Some(known) = &read {
                answers.recordings.insert(id.to_owned(), known.clone());
            }
        }
        read.ok_or(Verdict::Unavailable)
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

fn known_length(length: Option<i64>) -> Option<u64> {
    length
        .and_then(|length| u64::try_from(length).ok())
        .filter(|length| *length >= MIN_KNOWN_LENGTH_MS)
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
    use super::{
        Answers, Budget, Credit, Track, Verdict, corroborated, known_length, namesakes, normalize,
    };
    use crate::test_support::{
        musicbrainz::{FakeMusicBrainz, Reply},
        support,
    };
    use hito_metadata_musicbrainz::{ArtistRecording, ArtistRecordings, NameCandidate, NameSearch};
    use std::time::Duration;

    const ID: &str = "11111111-1111-4111-8111-111111111111";
    const OTHER: &str = "22222222-2222-4222-8222-222222222222";
    const VARIOUS_ARTISTS: &str = "89ad4ac3-39f7-470e-963a-56509c546377";

    fn page(names: &[(&str, &str, u32)], total: u32) -> NameSearch {
        NameSearch {
            total,
            items: names
                .iter()
                .map(|(id, name, score)| NameCandidate {
                    id: (*id).into(),
                    name: (*name).into(),
                    score: *score,
                })
                .collect(),
        }
    }

    #[test]
    fn names_compare_by_case_and_spacing_but_not_by_punctuation_or_accents() {
        assert_eq!(normalize("  Nova   QUARTET "), "nova quartet");
        assert_ne!(normalize("Nova Quartet"), normalize("Nova-Quartet"));
        assert_ne!(normalize("Beyoncé"), normalize("Beyonce"));
    }

    #[test]
    fn only_a_search_that_proves_it_saw_every_exact_name_names_any() {
        let exact = [(ID, "Nova Quartet", 100), (OTHER, "NOVA quartet", 100)];
        assert_eq!(
            namesakes(&page(&exact, 2), "Nova Quartet"),
            Some(vec![ID.into(), OTHER.into()])
        );
        let tail = [
            (ID, "Nova Quartet", 100),
            (OTHER, "Nova Quartet Tribute", 80),
        ];
        assert_eq!(
            namesakes(&page(&tail, 4_000), "Nova Quartet"),
            Some(vec![ID.into()]),
            "A lower score ends the maximum-relevance results however long the list"
        );
        assert_eq!(
            namesakes(&page(&exact, 150), "Nova Quartet"),
            None,
            "A page ending at the maximum score may hide an exact name"
        );
        let many = [
            (ID, "A", 100),
            (OTHER, "A", 100),
            ("33333333-3333-4333-8333-333333333333", "A", 100),
            ("44444444-4444-4444-8444-444444444444", "A", 100),
        ];
        assert_eq!(namesakes(&page(&many, 4), "A"), None);
        assert_eq!(
            namesakes(
                &page(&[(VARIOUS_ARTISTS, "Various Artists", 100)], 1),
                "Various Artists"
            ),
            Some(vec![])
        );
    }

    #[test]
    fn an_exact_name_scoring_below_the_maximum_makes_only_a_complete_search_conclusive() {
        let odd = [
            (ID, "Nova Quartet", 90),
            (OTHER, "Nova Quartet Tribute", 80),
        ];
        assert_eq!(
            namesakes(&page(&odd, 2), "Nova Quartet"),
            Some(vec![ID.into()]),
            "The whole result set is on the page"
        );
        assert_eq!(
            namesakes(&page(&odd, 400), "Nova Quartet"),
            None,
            "An exact name below 100 breaks the scoring assumption, so only a complete page counts"
        );
    }

    fn track(title: &str, length: u64) -> Track {
        Track {
            recording: title.into(),
            title: title.into(),
            length_ms: Some(length),
            observation: "o".into(),
            operation: None,
        }
    }

    fn listed(items: &[(&str, &str, u64)]) -> ArtistRecordings {
        ArtistRecordings {
            total: items.len() as u32,
            items: items
                .iter()
                .map(|(id, title, length)| ArtistRecording {
                    id: (*id).into(),
                    title: (*title).into(),
                    length_ms: Some(*length),
                })
                .collect(),
        }
    }

    #[test]
    fn two_recordings_sharing_one_title_do_not_corroborate() {
        let tracks = [track("Intro", 30_000), track("Intro", 90_000)];
        let (_, distinct) = corroborated(
            &tracks,
            &listed(&[("a", "Intro", 30_000), ("b", "Intro", 90_000)]),
        );
        assert_eq!(distinct, 1);
        let tracks = [track("Intro", 30_000), track("Outro", 90_000)];
        let (_, distinct) = corroborated(
            &tracks,
            &listed(&[("a", "Intro", 30_000), ("b", "outro", 90_000)]),
        );
        assert_eq!(distinct, 2);
    }

    #[test]
    fn a_missing_or_implausibly_short_library_length_is_unknown() {
        assert_eq!(known_length(Some(0)), None);
        assert_eq!(known_length(Some(999)), None);
        assert_eq!(known_length(Some(-5)), None);
        assert_eq!(known_length(None), None);
        assert_eq!(known_length(Some(1000)), Some(1000));
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
