use super::{
    ArtistProviderMatchReceipt, MatchArtistsFromProviderEvidence,
    apply::{self, EvidenceKind, Planned, REPLACEABLE, plan},
};
use crate::artist_catalog::{MetadataProvider, REQUEST_BUDGET, identity::receipts, policy};
use crate::{Actor, ArtistCatalog, Error, Result};
use hito_metadata_musicbrainz::IsrcRecording;
use sqlx::SqliteConnection;
use std::{
    collections::HashMap,
    time::{Duration, Instant},
};

const MAX_BATCH: i64 = 100;
const MAX_ID_LENGTH: usize = 64;
const MAX_LOOKUPS: usize = 8;
const MAX_CONSECUTIVE_FAILURES: usize = 3;
const LOOKUP_WINDOW: Duration = Duration::from_secs(9);
/// Special-purpose MusicBrainz artists that never identify a performer.
const SPECIAL_PURPOSE_ARTISTS: [&str; 4] = [
    "89ad4ac3-39f7-470e-963a-56509c546377",
    "125ec42a-7229-4250-afc5-e057484327fe",
    "eec63d3c-3b81-4ad4-b1e4-7c147d4d2b61",
    "f731ccc4-e22a-43af-a747-64213329e088",
];

struct Candidate {
    recording: String,
    isrc: Option<String>,
    page: Option<String>,
    observation: String,
    operation: Option<String>,
}

#[derive(Clone, PartialEq, Eq, Hash)]
enum Lookup {
    Isrc(String),
    Page(String),
}

#[derive(Clone)]
enum Resolution {
    Artist(String),
    Uncertain,
    Unavailable,
}

/// What the provider evidence of one recording concludes.
enum Outcome {
    Artist(String, EvidenceKind),
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
        let isrc = self.isrc.iter().cloned().map(Lookup::Isrc);
        let page = self.page.iter().cloned().map(Lookup::Page);
        isrc.chain(page).collect()
    }

    /// The ISRC decides first; a lone artist page is consulted only when the ISRC names no artist.
    fn outcome(&self, resolutions: &HashMap<Lookup, Resolution>) -> Outcome {
        let mut unavailable = false;
        for (lookup, kind) in self
            .isrc
            .iter()
            .map(|isrc| (Lookup::Isrc(isrc.clone()), EvidenceKind::Isrc))
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
    /// looked up nor changed.
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
        let empty = |next_after| ArtistProviderMatchReceipt {
            request_id: request.request_id,
            matched: 0,
            uncertain: 0,
            declined: 0,
            unavailable: 0,
            next_after,
        };
        let (mut candidates, mut next_after) = {
            let mut connection = self.store.pool.acquire().await?;
            actor.check_session_on(&mut connection).await?;
            if let Some(receipt) =
                receipts::replay(&mut connection, actor, request.request_id, &digest).await?
            {
                return Ok(receipt);
            }
            let settings = policy::read(&mut connection, actor).await?;
            if !policy::selected(&settings, &MetadataProvider::BuiltinMusicBrainz) {
                return Ok(empty(None));
            }
            candidates(&mut connection, actor, request.after.as_deref(), limit).await?
        };
        if candidates.is_empty() {
            return Ok(empty(next_after));
        }
        let (resolutions, examined) = {
            let _admission = self.admit(actor.id())?;
            self.resolve_lookups(&candidates).await
        };
        if examined < candidates.len() {
            next_after = Some(candidates[examined - 1].recording.clone());
            candidates.truncate(examined);
        }
        let mut tx = self.store.pool.begin_with("BEGIN IMMEDIATE").await?;
        actor.check_session_on(&mut tx).await?;
        if let Some(receipt) = receipts::replay(&mut tx, actor, request.request_id, &digest).await?
        {
            return Ok(receipt);
        }
        let settings = policy::read(&mut tx, actor).await?;
        if !policy::selected(&settings, &MetadataProvider::BuiltinMusicBrainz) {
            return Ok(empty(None));
        }
        let (mut uncertain, mut declined, mut unavailable) = (0, 0, 0);
        let mut pending = Vec::new();
        for candidate in &candidates {
            let remember = match candidate.outcome(&resolutions) {
                Outcome::Unavailable => {
                    unavailable += 1;
                    false
                }
                Outcome::Uncertain => {
                    uncertain += 1;
                    true
                }
                Outcome::Artist(artist, kind) => {
                    let current: Option<String> = sqlx::query_scalar(
                        "SELECT metadata_observation_id FROM recordings WHERE id=?",
                    )
                    .bind(&candidate.recording)
                    .fetch_optional(&mut *tx)
                    .await?
                    .flatten();
                    if current.as_deref() != Some(candidate.observation.as_str()) {
                        declined += 1;
                        false
                    } else {
                        match plan(
                            &mut tx,
                            actor,
                            &candidate.recording,
                            &artist,
                            settings.revision,
                            kind,
                        )
                        .await?
                        {
                            Planned::Match(item) => {
                                pending.push(*item);
                                false
                            }
                            Planned::Declined { remember } => {
                                declined += 1;
                                remember
                            }
                        }
                    }
                }
            };
            if remember {
                sqlx::query("INSERT INTO artist_provider_checks(user_id,recording_id,observation_id,artist_operation_id) VALUES(?,?,?,?)
                    ON CONFLICT(user_id,recording_id) DO UPDATE SET observation_id=excluded.observation_id,artist_operation_id=excluded.artist_operation_id")
                    .bind(actor.id()).bind(&candidate.recording).bind(&candidate.observation).bind(&candidate.operation)
                    .execute(&mut *tx).await?;
            }
        }
        let mut receipt = empty(next_after);
        receipt.matched = pending.len() as i64;
        receipt.uncertain = uncertain;
        receipt.declined = declined;
        receipt.unavailable = unavailable;
        if !pending.is_empty() {
            let order =
                receipts::save(&mut tx, actor, &digest, request.request_id, &receipt).await?;
            apply::commit(&mut tx, actor, &pending, order).await?;
        }
        actor.check_session_on(&mut tx).await?;
        tx.commit().await?;
        Ok(receipt)
    }

    /// Asks MusicBrainz once for every distinct code or page the candidates need, within the
    /// lookup and time bounds. An ISRC that names an artist spares its recording's page lookup.
    /// Returns how many leading candidates were examined.
    async fn resolve_lookups(
        &self,
        candidates: &[Candidate],
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
                    || started.elapsed() > LOOKUP_WINDOW)
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
                let resolution = self.ask(&lookup).await;
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

/// Recording, ISRC, lone artist page, current observation and personal artist operation.
type Row = (
    String,
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
        "SELECT r.id,i.isrc,
            (SELECT u.url FROM recording_artist_url_observations u
                WHERE u.observation_id=r.metadata_observation_id
                AND (SELECT count(*) FROM recording_artist_url_observations x WHERE x.observation_id=u.observation_id)=1),
            r.metadata_observation_id,p.artist_operation_id FROM recordings r
         JOIN library_members m ON m.library_id=r.library_id AND m.user_id=?
         LEFT JOIN recording_isrc_observations i ON i.observation_id=r.metadata_observation_id AND i.recording_id=r.id
         LEFT JOIN personal_recording_metadata p ON p.recording_id=r.id AND p.user_id=m.user_id
         WHERE r.id>?
            AND (i.observation_id IS NOT NULL OR (SELECT count(*) FROM recording_artist_url_observations x
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
            |(recording, isrc, page, observation, operation)| Candidate {
                recording,
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
    use super::{Resolution, SPECIAL_PURPOSE_ARTISTS, resolve};
    use hito_metadata_musicbrainz::IsrcRecording;

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
}
