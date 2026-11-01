use super::{
    ArtistProviderMatchReceipt, MatchArtistsFromProviderEvidence,
    apply::{EvidenceKind, REPLACEABLE},
    name_discography::NAME_CURSOR,
    settle::{Checks, Examined, Outcome, Settlement, empty_receipt},
};
use crate::artist_catalog::{MetadataProvider, REQUEST_BUDGET, identity::receipts, policy};
use crate::{Actor, ArtistCatalog, Error, Result};
use hito_metadata_musicbrainz::IsrcRecording;
use sqlx::SqliteConnection;
use std::{collections::HashMap, time::Duration};
use tokio::time::Instant;

const MAX_BATCH: i64 = 100;
const MAX_ID_LENGTH: usize = 64;
pub(crate) const MAX_LOOKUPS: usize = 8;
pub(crate) const MAX_CONSECUTIVE_FAILURES: usize = 3;
pub(crate) const LOOKUP_WINDOW: Duration = Duration::from_secs(9);
/// No lookup may still be running this long after the pass started, leaving room to write.
pub(crate) const LOOKUP_DEADLINE: Duration = Duration::from_secs(13);
/// Special-purpose MusicBrainz artists that never identify a performer.
pub(crate) const SPECIAL_PURPOSE_ARTISTS: [&str; 4] = [
    "89ad4ac3-39f7-470e-963a-56509c546377",
    "125ec42a-7229-4250-afc5-e057484327fe",
    "eec63d3c-3b81-4ad4-b1e4-7c147d4d2b61",
    "f731ccc4-e22a-43af-a747-64213329e088",
];

/// The recording identifier the current observation's file tags state, if it is well formed.
const TAGGED_RECORDING: &str = "(SELECT t.mbid FROM recording_musicbrainz_observations t
    WHERE t.observation_id=r.metadata_observation_id AND t.recording_id=r.id AND t.entity='recording')";

struct Candidate {
    recording: String,
    recording_id: Option<String>,
    isrc: Option<String>,
    page: Option<String>,
    observation: String,
    operation: Option<String>,
}

#[derive(Clone, PartialEq, Eq, Hash)]
enum Lookup {
    Recording(String),
    Isrc(String),
    Page(String),
}

enum Phase {
    Evidence(Vec<Candidate>, Option<String>),
    Names(String),
}

#[derive(Clone)]
enum Resolution {
    Artist(String),
    Uncertain,
    Unavailable,
}

fn single_artist(artists: &[String]) -> Resolution {
    match artists {
        [artist] if !SPECIAL_PURPOSE_ARTISTS.contains(&artist.as_str()) => {
            Resolution::Artist(artist.clone())
        }
        _ => Resolution::Uncertain,
    }
}

fn resolve(recordings: &[IsrcRecording]) -> Resolution {
    match recordings {
        [only] => single_artist(&only.artist_ids),
        _ => Resolution::Uncertain,
    }
}

impl Candidate {
    fn lookups(&self) -> Vec<Lookup> {
        let recording = self.recording_id.iter().cloned().map(Lookup::Recording);
        let isrc = self.isrc.iter().cloned().map(Lookup::Isrc);
        let page = self
            .page
            .iter()
            .filter(|page| hito_plugin_sdk::artist_page_provider(page).is_some())
            .cloned()
            .map(Lookup::Page);
        recording.chain(isrc).chain(page).collect()
    }

    /// The tagged recording decides first, then the ISRC; a lone artist page is consulted only
    /// when neither names an artist.
    fn outcome(&self, resolutions: &HashMap<Lookup, Resolution>) -> Outcome {
        let mut unavailable = false;
        for (lookup, kind) in self
            .recording_id
            .iter()
            .map(|id| (Lookup::Recording(id.clone()), EvidenceKind::RecordingId))
            .chain(
                self.isrc
                    .iter()
                    .map(|isrc| (Lookup::Isrc(isrc.clone()), EvidenceKind::Isrc)),
            )
            .chain(
                self.page
                    .iter()
                    .map(|page| (Lookup::Page(page.clone()), EvidenceKind::ProviderUrl)),
            )
        {
            match resolutions.get(&lookup) {
                Some(Resolution::Artist(artist)) => return Outcome::Artist(artist.clone(), kind),
                Some(Resolution::Unavailable) => unavailable = true,
                _ => {}
            }
        }
        if unavailable {
            Outcome::Unavailable
        } else {
            Outcome::Uncertain
        }
    }
}

impl ArtistCatalog {
    /// Matches recordings whose provider evidence resolves, through the built-in MusicBrainz
    /// service, to exactly one artist: the recording's own ISRC naming one recording credited to
    /// one artist, or its lone credited artist page being linked to one artist. Names never
    /// contribute evidence, and recordings the account has already decided about are neither
    /// looked up nor changed. When the evidence is exhausted the cursor continues into the name
    /// phase, which searches MusicBrainz by credit name and accepts an artist only when its own
    /// recordings corroborate the library.
    #[tracing::instrument(level = "info", skip_all, fields(operation = "artist_match_provider_evidence", request_id = %request.request_id))]
    pub async fn match_artists_from_provider_evidence(
        &self,
        actor: &Actor,
        request: MatchArtistsFromProviderEvidence,
    ) -> Result<ArtistProviderMatchReceipt> {
        let started = Instant::now();
        let result =
            tokio::time::timeout(REQUEST_BUDGET, self.accept_provider_matches(actor, request))
                .await
                .unwrap_or(Err(Error::Conflict(
                    "Artist matching took too long. Try again.".into(),
                )));
        match &result {
            Ok(receipt) => tracing::info!(
                outcome = "accepted",
                matched = receipt.matched,
                uncertain = receipt.uncertain,
                declined = receipt.declined,
                unavailable = receipt.unavailable,
                complete = receipt.next_after.is_none(),
                elapsed_ms = started.elapsed().as_millis() as u64,
                "Artist provider matching finished"
            ),
            Err(error) => tracing::info!(
                outcome = error.code(),
                elapsed_ms = started.elapsed().as_millis() as u64,
                "Artist provider matching finished"
            ),
        }
        result
    }

    async fn accept_provider_matches(
        &self,
        actor: &Actor,
        request: MatchArtistsFromProviderEvidence,
    ) -> Result<ArtistProviderMatchReceipt> {
        let limit = request.limit.unwrap_or(MAX_BATCH);
        if request.request_id.is_nil()
            || request
                .after
                .as_ref()
                .is_some_and(|after| after.len() > MAX_ID_LENGTH)
            || !(1..=MAX_BATCH).contains(&limit)
        {
            return Err(Error::Invalid(
                "Use a nonempty request and a batch of 1 to 100 recordings".into(),
            ));
        }
        let digest = receipts::fingerprint("hito-artist-match-provider:v1", "", &request)?;
        let phase = {
            let mut connection = self.store.pool.acquire().await?;
            actor.check_session_on(&mut connection).await?;
            if let Some(receipt) =
                receipts::replay(&mut connection, actor, request.request_id, &digest).await?
            {
                return Ok(receipt);
            }
            let settings = policy::read(&mut connection, actor).await?;
            if !policy::selected(&settings, &MetadataProvider::BuiltinMusicBrainz) {
                return Ok(empty_receipt(request.request_id, None));
            }
            match request
                .after
                .as_deref()
                .and_then(|after| after.strip_prefix(NAME_CURSOR))
            {
                Some(after) => Phase::Names(after.to_owned()),
                None => {
                    let (candidates, next_after) =
                        candidates(&mut connection, actor, request.after.as_deref(), limit).await?;
                    Phase::Evidence(candidates, next_after)
                }
            }
        };
        let (candidates, next_after) = match phase {
            Phase::Names(after) => {
                return self
                    .match_names(actor, request.request_id, &digest, &after)
                    .await;
            }
            Phase::Evidence(candidates, next_after) => (
                candidates,
                next_after.or_else(|| Some(NAME_CURSOR.to_owned())),
            ),
        };
        if candidates.is_empty() {
            return Ok(empty_receipt(request.request_id, next_after));
        }
        let (resolutions, examined) = {
            let _admission = self.admit_matching(actor)?;
            self.resolve_lookups(&candidates, LOOKUP_WINDOW).await
        };
        let mut next_after = next_after;
        if examined < candidates.len() {
            next_after = Some(candidates[examined - 1].recording.clone());
        }
        let examined = candidates
            .into_iter()
            .take(examined)
            .map(|candidate| Examined {
                outcome: candidate.outcome(&resolutions),
                recording: candidate.recording,
                observation: candidate.observation,
                operation: candidate.operation,
            })
            .collect();
        self.settle(
            actor,
            Settlement {
                request_id: request.request_id,
                digest: &digest,
                next_after,
                checks: Checks::Evidence,
                reviews: Vec::new(),
            },
            examined,
        )
        .await
    }

    /// Asks MusicBrainz once for every distinct code or page the candidates need, within the
    /// lookup and time bounds, which are checked before each lookup. An ISRC that names an artist
    /// spares its recording's page lookup. A lookup the budget refuses for the first candidate
    /// counts as unavailable; for a later one the candidate waits for the next batch. Returns how
    /// many leading candidates were examined.
    async fn resolve_lookups(
        &self,
        candidates: &[Candidate],
        window: Duration,
    ) -> (HashMap<Lookup, Resolution>, usize) {
        let started = Instant::now();
        let mut resolutions: HashMap<Lookup, Resolution> = HashMap::new();
        let (mut lookups, mut failures) = (0, 0);
        for (index, candidate) in candidates.iter().enumerate() {
            let needed = candidate
                .lookups()
                .iter()
                .filter(|lookup| !resolutions.contains_key(*lookup))
                .count();
            if lookups > 0
                && (lookups + needed > MAX_LOOKUPS
                    || failures >= MAX_CONSECUTIVE_FAILURES
                    || started.elapsed() > window)
            {
                return (resolutions, index);
            }
            for lookup in candidate.lookups() {
                if resolutions.contains_key(&lookup) {
                    if matches!(resolutions.get(&lookup), Some(Resolution::Artist(_))) {
                        break;
                    }
                    continue;
                }
                if lookups > 0
                    && (failures >= MAX_CONSECUTIVE_FAILURES || started.elapsed() > window)
                {
                    if index > 0 {
                        return (resolutions, index);
                    }
                    resolutions.insert(lookup, Resolution::Unavailable);
                    continue;
                }
                let remaining = LOOKUP_DEADLINE.saturating_sub(started.elapsed());
                let resolution = tokio::time::timeout(remaining, self.ask(&lookup))
                    .await
                    .unwrap_or(Resolution::Unavailable);
                lookups += 1;
                failures = if matches!(resolution, Resolution::Unavailable) {
                    failures + 1
                } else {
                    0
                };
                let decisive = matches!(resolution, Resolution::Artist(_));
                resolutions.insert(lookup, resolution);
                if decisive {
                    break;
                }
            }
        }
        (resolutions, candidates.len())
    }

    async fn ask(&self, lookup: &Lookup) -> Resolution {
        let answer = match lookup {
            Lookup::Recording(id) => self.builtin.recording_artists(id).await.map(|found| {
                found.map_or(Resolution::Uncertain, |found| {
                    single_artist(&found.artist_ids)
                })
            }),
            Lookup::Isrc(isrc) => self
                .builtin
                .recordings_by_isrc(isrc)
                .await
                .map(|recordings| resolve(&recordings)),
            Lookup::Page(page) => self
                .builtin
                .artists_by_url(page)
                .await
                .map(|artists| single_artist(&artists)),
        };
        answer.unwrap_or(Resolution::Unavailable)
    }
}

/// Recording, tagged MusicBrainz recording, ISRC, lone artist page, current observation and
/// personal artist operation.
type Row = (
    String,
    Option<String>,
    Option<String>,
    Option<String>,
    String,
    Option<String>,
);

async fn candidates(
    connection: &mut SqliteConnection,
    actor: &Actor,
    after: Option<&str>,
    limit: i64,
) -> Result<(Vec<Candidate>, Option<String>)> {
    let mut rows: Vec<Row> =
        sqlx::query_as(&format!(
        "SELECT r.id,{TAGGED_RECORDING},i.isrc,
            (SELECT u.url FROM recording_artist_url_observations u
                WHERE u.observation_id=r.metadata_observation_id
                AND (SELECT count(*) FROM recording_artist_url_observations x WHERE x.observation_id=u.observation_id)=1),
            r.metadata_observation_id,p.artist_operation_id FROM recordings r
         JOIN library_members m ON m.library_id=r.library_id AND m.user_id=?
         LEFT JOIN recording_isrc_observations i ON i.observation_id=r.metadata_observation_id AND i.recording_id=r.id
         LEFT JOIN personal_recording_metadata p ON p.recording_id=r.id AND p.user_id=m.user_id
         WHERE r.id>?
            AND (i.observation_id IS NOT NULL OR {TAGGED_RECORDING} IS NOT NULL OR (SELECT count(*) FROM recording_artist_url_observations x
                WHERE x.observation_id=r.metadata_observation_id)=1)
            AND NOT EXISTS (SELECT 1 FROM artist_provider_checks k WHERE k.user_id=m.user_id
                AND k.recording_id=r.id AND k.observation_id=r.metadata_observation_id
                AND k.artist_operation_id IS p.artist_operation_id)
            AND NOT EXISTS (SELECT 1 FROM recording_artist_identity_choices c
                WHERE c.user_id=m.user_id AND c.recording_id=r.id AND NOT {REPLACEABLE})
         ORDER BY r.id LIMIT ?"
    ))
    .bind(actor.id())
    .bind(after.unwrap_or_default())
    .bind(limit + 1)
    .fetch_all(connection)
    .await?;
    let next_after = if rows.len() as i64 > limit {
        rows.truncate(limit as usize);
        rows.last().map(|row| row.0.clone())
    } else {
        None
    };
    let candidates = rows
        .into_iter()
        .map(
            |(recording, recording_id, isrc, page, observation, operation)| Candidate {
                recording,
                recording_id,
                isrc,
                page,
                observation,
                operation,
            },
        )
        .collect();
    Ok((candidates, next_after))
}

#[cfg(test)]
mod tests {
    use super::{Candidate, Lookup, Resolution, SPECIAL_PURPOSE_ARTISTS, resolve};
    use crate::test_support::{
        musicbrainz::{FakeMusicBrainz, Reply},
        support,
    };
    use hito_metadata_musicbrainz::IsrcRecording;
    use std::time::Duration;

    const PAGE: &str = "https://open.spotify.com/artist/0000000000000000000001";

    const A: &str = "11111111-1111-4111-8111-111111111111";
    const B: &str = "22222222-2222-4222-8222-222222222222";

    fn recording(artists: &[&str]) -> IsrcRecording {
        IsrcRecording {
            id: "0f700000-0000-4000-8000-0000000000a1".into(),
            artist_ids: artists.iter().map(|id| (*id).into()).collect(),
        }
    }

    #[test]
    fn only_one_recording_with_one_ordinary_credited_artist_names_an_artist() {
        assert!(matches!(resolve(&[recording(&[A])]), Resolution::Artist(id) if id == A));
        assert!(matches!(resolve(&[]), Resolution::Uncertain));
        assert!(matches!(
            resolve(&[recording(&[A]), recording(&[A])]),
            Resolution::Uncertain
        ));
        assert!(matches!(
            resolve(&[recording(&[A, B])]),
            Resolution::Uncertain
        ));
        assert!(matches!(resolve(&[recording(&[])]), Resolution::Uncertain));
        for special in SPECIAL_PURPOSE_ARTISTS {
            assert!(matches!(
                resolve(&[recording(&[special])]),
                Resolution::Uncertain
            ));
        }
    }

    fn candidate(isrc: &str, page: Option<&str>) -> Candidate {
        Candidate {
            recording: isrc.into(),
            recording_id: None,
            isrc: Some(isrc.into()),
            page: page.map(str::to_owned),
            observation: "observation".into(),
            operation: None,
        }
    }

    async fn catalog(fake: &FakeMusicBrainz) -> crate::ArtistCatalog {
        let (_directory, hito, ..) =
            support::library_with_musicbrainz("lookup-budget", Some(fake.endpoint.clone())).await;
        hito.artist_catalog
    }

    #[tokio::test]
    async fn the_window_is_checked_before_every_lookup_of_the_first_candidate() {
        let fake = FakeMusicBrainz::start().await;
        fake.isrc("GBAAA2000001", Reply::NotFound);
        fake.isrc("GBAAA2000002", Reply::NotFound);
        fake.url(PAGE, Reply::Artists(vec![]));
        fake.delay_replies(Duration::from_millis(600));
        let catalog = catalog(&fake).await;
        let candidates = [
            candidate("GBAAA2000001", Some(PAGE)),
            candidate("GBAAA2000002", None),
        ];
        let (resolutions, examined) = catalog
            .resolve_lookups(&candidates, Duration::from_millis(500))
            .await;
        assert_eq!(examined, 1);
        assert_eq!(fake.requests().len(), 1, "{:?}", fake.requests());
        assert!(matches!(
            resolutions.get(&Lookup::Page(PAGE.into())),
            Some(Resolution::Unavailable)
        ));
    }

    #[tokio::test]
    async fn a_later_candidate_whose_second_lookup_is_over_the_window_waits_for_the_next_batch() {
        let fake = FakeMusicBrainz::start().await;
        fake.isrc("GBAAA2000001", Reply::NotFound);
        fake.isrc("GBAAA2000002", Reply::NotFound);
        fake.url(PAGE, Reply::Artists(vec![]));
        let catalog = catalog(&fake).await;
        let candidates = [
            candidate("GBAAA2000001", None),
            candidate("GBAAA2000002", Some(PAGE)),
        ];
        let (_, examined) = catalog
            .resolve_lookups(&candidates, Duration::from_millis(500))
            .await;
        assert_eq!(examined, 1);
        assert_eq!(fake.requests().len(), 2, "{:?}", fake.requests());
    }
}
