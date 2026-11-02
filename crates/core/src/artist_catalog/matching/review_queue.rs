use super::{
    ArtistReviewCandidate, ArtistReviewConfirmation, ArtistReviewDismissal, ArtistReviewItem,
    ArtistReviewOrigin, ArtistReviewQueueQuery, ArtistReviewRestoration,
    ConfirmArtistReviewCandidate, DismissArtistReviewCandidates, RestoreArtistReviewCandidates,
    apply::{AUTOMATIC_CHOICE, EvidenceKind, Planned, REPLACEABLE, commit, plan_for_credit},
    coverage::MIN_COUNTING_RULE,
    name_discography::NAME_RULE,
    name_probe::{Assessment, Candidate, distinct_titles},
    suggestions::credit_name,
};
use crate::artist_catalog::{
    LINKED, MetadataProvider, REQUEST_BUDGET,
    identity::{
        groups::{current_choices, current_choices_joined},
        receipts,
    },
    policy,
};
use crate::{Actor, ArtistCatalog, Error, Page, Result};
use hito_metadata_musicbrainz::NameCandidate;
use sqlx::SqliteConnection;
use std::{
    collections::{HashMap, HashSet},
    time::Instant,
};
use uuid::Uuid;

const DEFAULT_LIMIT: i64 = 20;
const MAX_LIMIT: i64 = 50;
const MAX_OFFSET: i64 = 100_000;
const MAX_CANDIDATES: i64 = 3;
const MAX_CHOSEN_OFFERS: i64 = 50;
const MAX_APPLIED: i64 = 200;
const MAX_ID_LENGTH: usize = 64;
const MAX_TEXT_CHARS: usize = 200;

pub(super) struct ReviewCandidate {
    id: String,
    name: String,
    kind: Option<String>,
    disambiguation: Option<String>,
    country: Option<String>,
    titles: i64,
}

fn clipped(text: &str, limit: usize) -> String {
    text.chars().take(limit).collect()
}

pub(super) fn candidates(
    ids: &[String],
    assessments: &[(String, Assessment)],
    tracks: &[Candidate],
    described: &HashMap<String, NameCandidate>,
) -> Vec<ReviewCandidate> {
    ids.iter()
        .filter_map(|id| {
            let found = described.get(id)?;
            let titles =
                assessments
                    .iter()
                    .find(|(known, _)| known == id)
                    .map_or(0, |(_, assessment)| match assessment {
                        Assessment::Corroborated(hits) | Assessment::Absent(hits) => {
                            distinct_titles(tracks, hits)
                        }
                        Assessment::NotRead => 0,
                    });
            Some(ReviewCandidate {
                id: id.clone(),
                name: clipped(&found.name, MAX_TEXT_CHARS),
                kind: found.kind.as_deref().map(|kind| clipped(kind, 24)),
                disambiguation: found
                    .disambiguation
                    .as_deref()
                    .map(|text| clipped(text, MAX_TEXT_CHARS)),
                country: found.country.as_deref().map(|code| clipped(code, 8)),
                titles: titles as i64,
            })
        })
        .collect()
}

pub(super) async fn keep(
    connection: &mut SqliteConnection,
    actor: &Actor,
    credit: &str,
    found: &[ReviewCandidate],
) -> Result<()> {
    sqlx::query("DELETE FROM artist_name_candidates WHERE user_id=? AND artist_id=?")
        .bind(actor.id())
        .bind(credit)
        .execute(&mut *connection)
        .await?;
    for (position, candidate) in found.iter().enumerate() {
        sqlx::query(
            "INSERT INTO artist_name_candidates(user_id,artist_id,provider_artist_id,position,name,kind,disambiguation,country,corroborated_titles)
             VALUES(?,?,?,?,?,?,?,?,?)",
        )
        .bind(actor.id())
        .bind(credit)
        .bind(&candidate.id)
        .bind(position as i64)
        .bind(&candidate.name)
        .bind(&candidate.kind)
        .bind(&candidate.disambiguation)
        .bind(&candidate.country)
        .bind(candidate.titles)
        .execute(&mut *connection)
        .await?;
    }
    Ok(())
}

fn undecided(credit_filter: &str) -> String {
    format!(
        "SELECT r.id AS id,rc.artist_id AS artist_id FROM artist_name_checks k
         JOIN recordings r ON r.id=k.recording_id AND r.metadata_observation_id=k.observation_id
         JOIN account_recording_credits rc ON rc.recording_id=r.id AND rc.user_id=k.user_id
         JOIN library_members m ON m.library_id=r.library_id AND m.user_id=k.user_id
         LEFT JOIN personal_recording_metadata p ON p.recording_id=r.id AND p.user_id=k.user_id
         WHERE k.user_id=?1 AND k.rule={NAME_RULE} AND p.artist_operation_id IS NULL
            AND k.artist_operation_id IS NULL {credit_filter}
            AND NOT EXISTS (SELECT 1 FROM recording_artist_identity_choices c
                WHERE c.user_id=k.user_id AND c.recording_id=r.id AND NOT {REPLACEABLE})"
    )
}

fn edited(credit_filter: &str) -> String {
    format!(
        "SELECT r.id AS id,rc.artist_id AS artist_id FROM personal_recording_metadata p
         CROSS JOIN recordings r ON r.id=p.recording_id AND r.metadata_observation_id IS NOT NULL
         CROSS JOIN account_recording_credits rc ON rc.recording_id=r.id AND rc.user_id=p.user_id
         CROSS JOIN library_members m ON m.library_id=r.library_id AND m.user_id=p.user_id
         WHERE p.user_id=?1 AND p.artist_operation_id IS NOT NULL {credit_filter}
            AND NOT EXISTS (SELECT 1 FROM recording_artist_identity_choices c
                WHERE c.user_id=p.user_id AND c.recording_id=r.id AND NOT {REPLACEABLE})"
    )
}

fn tracks_of(searched: bool, chosen: bool, credit_filter: &str) -> String {
    let mut parts = Vec::new();
    if searched {
        parts.push(format!("SELECT id FROM ({})", undecided(credit_filter)));
    }
    if chosen {
        parts.push(format!("SELECT id FROM ({})", edited(credit_filter)));
    }
    parts.join(" UNION ALL ")
}

fn chosen_names() -> String {
    format!(
        "SELECT lower(trim(artist.name)) AS name_key,g.provider_artist_id AS provider_artist_id
         {} ?1 AND g.provider_kind='builtin_musicbrainz'
         GROUP BY name_key,g.provider_artist_id",
        current_choices_joined(
            "JOIN account_artist_identity_groups g ON g.user_id=choice.user_id AND g.id=choice.group_id"
        )
    )
}

struct ChosenOffer {
    provider_artist_id: String,
    name: String,
}

async fn chosen_offers(
    connection: &mut SqliteConnection,
    actor: &Actor,
    credit: &str,
) -> Result<Vec<ChosenOffer>> {
    let rows: Vec<(String, String)> = sqlx::query_as(&format!(
        "SELECT provider_artist_id,name FROM (
            SELECT (SELECT g.provider_artist_id FROM account_artist_identity_groups g
                    WHERE g.user_id=choice.user_id AND g.id=choice.group_id AND g.provider_kind='builtin_musicbrainz') AS provider_artist_id,
                min(artist.name) AS name,count(*) AS tracks
            {} ?1 AND lower(trim(artist.name))=(SELECT lower(trim(name)) FROM account_artists WHERE user_id=?1 AND id=?2)
            GROUP BY choice.group_id)
         WHERE provider_artist_id IS NOT NULL ORDER BY tracks DESC,provider_artist_id LIMIT ?3",
        current_choices()
    ))
    .bind(actor.id())
    .bind(credit)
    .bind(MAX_CHOSEN_OFFERS)
    .fetch_all(connection)
    .await?;
    Ok(rows
        .into_iter()
        .map(|(provider_artist_id, name)| ChosenOffer {
            provider_artist_id,
            name,
        })
        .collect())
}

fn matched_basis(credit_filter: &str) -> String {
    format!(
        "FROM account_recording_credits credit
         JOIN recording_artist_identity_choices choice ON choice.user_id=credit.user_id
            AND choice.recording_id=credit.recording_id AND choice.artist_id=credit.artist_id
         JOIN account_artist_identity_groups g ON g.user_id=choice.user_id AND g.id=choice.group_id
            AND g.provider_kind='builtin_musicbrainz'
         JOIN recordings r ON r.id=credit.recording_id
         JOIN library_members m ON m.library_id=r.library_id AND m.user_id=credit.user_id
         LEFT JOIN personal_recording_metadata p ON p.recording_id=r.id AND p.user_id=credit.user_id
         WHERE credit.user_id=?1 {credit_filter} AND choice.retired=0 AND choice.association_id IS NOT NULL
            AND choice.artist_operation_id IS NULL AND p.artist_operation_id IS NULL
            AND r.metadata_observation_id=choice.observation_id AND {AUTOMATIC_CHOICE}
            AND NOT EXISTS (SELECT 1 FROM artist_association_name_matches weak
                WHERE weak.association_id=choice.association_id AND weak.rule<{MIN_COUNTING_RULE})
            AND EXISTS (SELECT 1 FROM recording_artist_associations assoc WHERE assoc.id=choice.association_id AND {LINKED})"
    )
}

pub(super) fn matched_counts(credit_filter: &str) -> String {
    format!(
        "SELECT credit.artist_id AS artist_id,g.provider_artist_id AS provider_artist_id,count(*) AS tracks
         {} GROUP BY credit.artist_id,g.provider_artist_id",
        matched_basis(credit_filter)
    )
}

fn matched_state() -> String {
    format!(
        "(SELECT COALESCE(1+min(dismissed)+2*max(dismissed),0) FROM (
            SELECT EXISTS (SELECT 1 FROM artist_review_dismissals d WHERE d.user_id=?1
                AND d.artist_id=credit.artist_id AND d.provider_artist_id=g.provider_artist_id) AS dismissed
            {}))",
        matched_basis("AND credit.artist_id=base.id")
    )
}

struct MatchedOffer {
    provider_artist_id: String,
    tracks: i64,
}

async fn matched_offers(
    connection: &mut SqliteConnection,
    actor: &Actor,
    credit: &str,
) -> Result<Vec<MatchedOffer>> {
    let rows: Vec<(String, i64)> = sqlx::query_as(&format!(
        "SELECT provider_artist_id,tracks FROM ({})
         ORDER BY tracks DESC,provider_artist_id LIMIT ?3",
        matched_counts("AND credit.artist_id=?2")
    ))
    .bind(actor.id())
    .bind(credit)
    .bind(MAX_CHOSEN_OFFERS)
    .fetch_all(connection)
    .await?;
    Ok(rows
        .into_iter()
        .map(|(provider_artist_id, tracks)| MatchedOffer {
            provider_artist_id,
            tracks,
        })
        .collect())
}

async fn offered_ids(
    connection: &mut SqliteConnection,
    actor: &Actor,
    credit: &str,
) -> Result<Vec<String>> {
    let mut ids: Vec<String> = chosen_offers(connection, actor, credit)
        .await?
        .into_iter()
        .map(|offer| offer.provider_artist_id)
        .collect();
    for offer in matched_offers(connection, actor, credit).await? {
        if !ids.contains(&offer.provider_artist_id) {
            ids.push(offer.provider_artist_id);
        }
    }
    Ok(ids)
}

async fn dismiss_one(
    connection: &mut SqliteConnection,
    actor: &Actor,
    credit: &str,
    candidate: &str,
) -> Result<u64> {
    Ok(sqlx::query(
        "INSERT OR IGNORE INTO artist_review_dismissals(user_id,artist_id,provider_artist_id) VALUES(?,?,?)",
    )
    .bind(actor.id())
    .bind(credit)
    .bind(candidate)
    .execute(connection)
    .await?
    .rows_affected())
}

async fn dismissed_ids(
    connection: &mut SqliteConnection,
    actor: &Actor,
    credit: &str,
) -> Result<HashSet<String>> {
    Ok(sqlx::query_scalar::<_, String>(
        "SELECT provider_artist_id FROM artist_review_dismissals WHERE user_id=? AND artist_id=?",
    )
    .bind(actor.id())
    .bind(credit)
    .fetch_all(connection)
    .await?
    .into_iter()
    .collect())
}

async fn undismissed_offer(
    connection: &mut SqliteConnection,
    actor: &Actor,
    credit: &str,
    candidate: &str,
) -> Result<bool> {
    let offered = chosen_offers(connection, actor, credit)
        .await?
        .iter()
        .any(|offer| offer.provider_artist_id == candidate);
    Ok(offered
        && !dismissed_ids(connection, actor, credit)
            .await?
            .contains(candidate))
}

const DISMISSED_MATCH: &str = "EXISTS (SELECT 1 FROM artist_review_dismissals d
    WHERE d.user_id=c.user_id AND d.artist_id=c.artist_id AND d.provider_artist_id=c.provider_artist_id)";
const OFFERED: &str = "NOT EXISTS (SELECT 1 FROM artist_review_dismissals d
    WHERE d.user_id=c.user_id AND d.artist_id=c.artist_id AND d.provider_artist_id=c.provider_artist_id)";

#[derive(Clone, Copy)]
pub(super) enum Listing {
    Offered,
    Dismissed,
}

impl Listing {
    fn filter(self) -> &'static str {
        match self {
            Self::Offered => OFFERED,
            Self::Dismissed => DISMISSED_MATCH,
        }
    }
}

pub(super) fn queued(listing: Listing) -> String {
    let filter = listing.filter();
    let stored = format!(
        "EXISTS (SELECT 1 FROM artist_name_candidates c
            WHERE c.user_id=a.user_id AND c.artist_id=a.id AND {filter})"
    );
    let state = matched_state();
    let (unlisted, matched) = match listing {
        Listing::Offered => ("has_stored=0 AND state=0", "state IN (1,3)"),
        Listing::Dismissed => ("0", "state IN (3,4)"),
    };
    let dismissal = "EXISTS (SELECT 1 FROM artist_review_dismissals d WHERE d.user_id=a.user_id
        AND d.artist_id=a.id AND d.provider_artist_id=n.provider_artist_id)";
    let offer_dismissal = match listing {
        Listing::Offered => format!("NOT {dismissal}"),
        Listing::Dismissed => dismissal.to_owned(),
    };
    let chosen = format!(
        "EXISTS (SELECT 1 FROM chosen_names n WHERE n.name_key=lower(trim(a.name)) AND {offer_dismissal})"
    );
    format!(
        "WITH undecided AS (SELECT artist_id,count(*) AS tracks FROM ({}) GROUP BY artist_id),
              chosen_names AS MATERIALIZED ({}),
              edited AS (SELECT artist_id,count(*) AS tracks FROM ({}) GROUP BY artist_id),
              base AS MATERIALIZED (
                SELECT a.id AS id,a.name AS name,COALESCE(u.tracks,0) AS undecided_tracks,
                    COALESCE(e.tracks,0) AS edited_tracks,
                    CASE WHEN u.tracks>0 THEN {stored} ELSE 0 END AS stored,
                    CASE WHEN u.tracks>0 THEN EXISTS (SELECT 1 FROM artist_name_candidates c
                        WHERE c.user_id=a.user_id AND c.artist_id=a.id) ELSE 0 END AS has_stored,
                    CASE WHEN e.tracks>0 THEN {chosen} ELSE 0 END AS chosen
                FROM account_artists a
                JOIN library_members m ON m.library_id=a.library_id AND m.user_id=a.user_id
                LEFT JOIN undecided u ON u.artist_id=a.id
                LEFT JOIN edited e ON e.artist_id=a.id
                WHERE a.user_id=?1 AND (u.tracks>0 OR e.tracks>0)),
              flags AS MATERIALIZED (
                SELECT id,name,undecided_tracks,edited_tracks,stored,has_stored,chosen,
                    CASE WHEN undecided_tracks>0 AND stored=0 THEN {state} ELSE 0 END AS state
                FROM base),
              verdicts AS (
                SELECT id,name,undecided_tracks,edited_tracks,stored,chosen,
                    {unlisted} AND undecided_tracks>0 AS unlisted,{matched} AS matched
                FROM flags)
         SELECT id,name,
            CASE WHEN stored OR unlisted OR matched THEN undecided_tracks ELSE 0 END
                + CASE WHEN chosen THEN edited_tracks ELSE 0 END AS tracks,
            stored,chosen,matched
         FROM verdicts WHERE stored OR unlisted OR matched OR chosen",
        undecided(""),
        chosen_names(),
        edited("")
    )
}

impl ArtistCatalog {
    #[tracing::instrument(level = "debug", skip_all, fields(operation = "artist_review_queue", request_id = %Uuid::new_v4()))]
    pub async fn artist_review_queue(
        &self,
        actor: &Actor,
        query: ArtistReviewQueueQuery,
    ) -> Result<Page<ArtistReviewItem>> {
        let started = Instant::now();
        let result = tokio::time::timeout(
            REQUEST_BUDGET,
            self.read_review_queue(actor, query, Listing::Offered),
        )
        .await
        .unwrap_or(Err(Error::Conflict(
            "The review queue took too long. Try again.".into(),
        )));
        tracing::debug!(
            outcome = result.as_ref().map_or_else(|e| e.code(), |_| "success"),
            credits = result.as_ref().map_or(0, |page| page.items.len()),
            elapsed_ms = started.elapsed().as_millis() as u64,
            "Artist review queue loaded"
        );
        result
    }

    #[tracing::instrument(level = "debug", skip_all, fields(operation = "artist_review_dismissed", request_id = %Uuid::new_v4()))]
    pub async fn dismissed_artist_reviews(
        &self,
        actor: &Actor,
        query: ArtistReviewQueueQuery,
    ) -> Result<Page<ArtistReviewItem>> {
        let started = Instant::now();
        let result = tokio::time::timeout(
            REQUEST_BUDGET,
            self.read_review_queue(actor, query, Listing::Dismissed),
        )
        .await
        .unwrap_or(Err(Error::Conflict(
            "The dismissed suggestions took too long. Try again.".into(),
        )));
        tracing::debug!(
            outcome = result.as_ref().map_or_else(|e| e.code(), |_| "success"),
            credits = result.as_ref().map_or(0, |page| page.items.len()),
            elapsed_ms = started.elapsed().as_millis() as u64,
            "Dismissed artist reviews loaded"
        );
        result
    }

    async fn read_review_queue(
        &self,
        actor: &Actor,
        query: ArtistReviewQueueQuery,
        listing: Listing,
    ) -> Result<Page<ArtistReviewItem>> {
        let limit = query.limit.unwrap_or(DEFAULT_LIMIT);
        let offset = query.offset.unwrap_or(0);
        if !(1..=MAX_LIMIT).contains(&limit) || !(0..=MAX_OFFSET).contains(&offset) {
            return Err(Error::Invalid(
                "Use a page of 1 to 50 credits and a smaller offset".into(),
            ));
        }
        let mut tx = self.store.pool.begin().await?;
        actor.check_session_on(&mut tx).await?;
        let base = queued(listing);
        let filter = listing.filter();
        let rows: Vec<(String, String, i64, i64, i64, i64, i64)> = sqlx::query_as(&format!(
            "SELECT id,name,tracks,stored,chosen,matched,count(*) OVER () FROM ({base})
             ORDER BY (stored+chosen+matched)=0,tracks DESC,name,id LIMIT ?2 OFFSET ?3"
        ))
        .bind(actor.id())
        .bind(limit)
        .bind(offset)
        .fetch_all(&mut *tx)
        .await?;
        let total = match rows.first() {
            Some(first) => first.6,
            None if offset == 0 => 0,
            None => {
                sqlx::query_scalar(&format!("SELECT count(*) FROM ({base})"))
                    .bind(actor.id())
                    .fetch_one(&mut *tx)
                    .await?
            }
        };
        let searching: Vec<&str> = rows
            .iter()
            .filter(|row| row.3 == 0 && row.4 == 0 && row.5 == 0)
            .map(|row| row.0.as_str())
            .collect();
        let search_ids: HashMap<String, String> = if searching.is_empty() {
            HashMap::new()
        } else {
            sqlx::query_as::<_, (String, String)>(&format!(
                "SELECT artist_id,min(id) FROM ({}) GROUP BY artist_id",
                undecided("AND rc.artist_id IN (SELECT value FROM json_each(?2))")
            ))
            .bind(actor.id())
            .bind(
                serde_json::to_string(&searching)
                    .map_err(|_| Error::Internal("Search list".into()))?,
            )
            .fetch_all(&mut *tx)
            .await?
            .into_iter()
            .collect()
        };
        let matching: Vec<&str> = rows
            .iter()
            .filter(|row| row.3 > 0 || row.5 > 0)
            .map(|row| row.0.as_str())
            .collect();
        let mut matched_by_credit: HashMap<String, Vec<MatchedOffer>> = HashMap::new();
        if !matching.is_empty() {
            let counts: Vec<(String, String, i64)> = sqlx::query_as(&format!(
                "SELECT artist_id,provider_artist_id,tracks FROM ({})
                 ORDER BY artist_id,tracks DESC,provider_artist_id",
                matched_counts("AND credit.artist_id IN (SELECT value FROM json_each(?2))")
            ))
            .bind(actor.id())
            .bind(
                serde_json::to_string(&matching)
                    .map_err(|_| Error::Internal("Matched list".into()))?,
            )
            .fetch_all(&mut *tx)
            .await?;
            for (artist_id, provider_artist_id, tracks) in counts {
                matched_by_credit
                    .entry(artist_id)
                    .or_default()
                    .push(MatchedOffer {
                        provider_artist_id,
                        tracks,
                    });
            }
        }
        let mut items = Vec::with_capacity(rows.len());
        for (artist_id, name, undecided_tracks, stored, chosen, matched, _) in rows {
            let mut candidates: Vec<ArtistReviewCandidate> = Vec::new();
            let found: Vec<ArtistReviewCandidate> = if stored > 0 {
                sqlx::query_as(&format!(
                    "SELECT c.provider_artist_id,c.name,c.kind,c.disambiguation,c.country,
                            c.corroborated_titles,
                            EXISTS (SELECT 1 FROM account_artist_identity_groups g WHERE g.user_id=c.user_id
                                AND g.provider_kind='builtin_musicbrainz' AND g.provider_artist_id=c.provider_artist_id) AS in_library
                     FROM artist_name_candidates c WHERE c.user_id=? AND c.artist_id=? AND {filter}
                     ORDER BY in_library DESC,c.corroborated_titles DESC,c.position LIMIT ?"
                ))
                .bind(actor.id())
                .bind(&artist_id)
                .bind(MAX_CANDIDATES)
                .fetch_all(&mut *tx)
                .await?
            } else {
                Vec::new()
            };
            let dismissed = if chosen > 0 || matched > 0 || stored > 0 {
                dismissed_ids(&mut tx, actor, &artist_id).await?
            } else {
                HashSet::new()
            };
            if chosen > 0 {
                for offer in chosen_offers(&mut tx, actor, &artist_id).await? {
                    let is_dismissed = dismissed.contains(&offer.provider_artist_id);
                    if is_dismissed != matches!(listing, Listing::Dismissed) {
                        continue;
                    }
                    let same = found
                        .iter()
                        .find(|stored| stored.provider_artist_id == offer.provider_artist_id);
                    candidates.push(ArtistReviewCandidate {
                        provider_artist_id: offer.provider_artist_id,
                        name: clipped(&offer.name, MAX_TEXT_CHARS),
                        kind: same.and_then(|stored| stored.kind.clone()),
                        disambiguation: same.and_then(|stored| stored.disambiguation.clone()),
                        country: same.and_then(|stored| stored.country.clone()),
                        corroborated_titles: same.map_or(0, |stored| stored.corroborated_titles),
                        in_library: true,
                        origin: ArtistReviewOrigin::ChosenArtist,
                    });
                }
            }
            if stored > 0 || matched > 0 {
                let offers = matched_by_credit.remove(&artist_id).unwrap_or_default();
                for offer in offers.into_iter().take(MAX_CHOSEN_OFFERS as usize) {
                    let is_dismissed = dismissed.contains(&offer.provider_artist_id);
                    if is_dismissed != matches!(listing, Listing::Dismissed)
                        || candidates
                            .iter()
                            .any(|known| known.provider_artist_id == offer.provider_artist_id)
                    {
                        continue;
                    }
                    let same = found
                        .iter()
                        .find(|stored| stored.provider_artist_id == offer.provider_artist_id);
                    candidates.push(ArtistReviewCandidate {
                        provider_artist_id: offer.provider_artist_id,
                        name: same.map_or_else(
                            || clipped(&name, MAX_TEXT_CHARS),
                            |stored| stored.name.clone(),
                        ),
                        kind: same.and_then(|stored| stored.kind.clone()),
                        disambiguation: same.and_then(|stored| stored.disambiguation.clone()),
                        country: same.and_then(|stored| stored.country.clone()),
                        corroborated_titles: offer.tracks,
                        in_library: true,
                        origin: ArtistReviewOrigin::MatchedOnOtherTracks,
                    });
                }
            }
            for stored in found {
                if !candidates
                    .iter()
                    .any(|known| known.provider_artist_id == stored.provider_artist_id)
                {
                    candidates.push(stored);
                }
            }
            candidates.truncate(MAX_CANDIDATES as usize);
            items.push(ArtistReviewItem {
                search_recording_id: search_ids.get(&artist_id).cloned(),
                artist_id,
                name,
                undecided_tracks,
                needs_search: candidates.is_empty(),
                candidates,
            });
        }
        actor.check_session_on(&mut tx).await?;
        tx.commit().await?;
        Ok(Page {
            items,
            total,
            offset,
            limit,
        })
    }

    #[tracing::instrument(level = "info", skip_all, fields(operation = "artist_review_confirm", request_id = %request.request_id))]
    pub async fn confirm_artist_review_candidate(
        &self,
        actor: &Actor,
        artist: &str,
        request: ConfirmArtistReviewCandidate,
    ) -> Result<ArtistReviewConfirmation> {
        let started = Instant::now();
        let result = self.apply_review_candidate(actor, artist, request).await;
        match &result {
            Ok(receipt) => tracing::info!(
                outcome = "confirmed",
                applied = receipt.applied,
                kept = receipt.kept,
                remaining = receipt.remaining,
                elapsed_ms = started.elapsed().as_millis() as u64,
                "Artist review candidate confirmed"
            ),
            Err(error) => tracing::info!(
                outcome = error.code(),
                elapsed_ms = started.elapsed().as_millis() as u64,
                "Artist review candidate confirmed"
            ),
        }
        result
    }

    async fn apply_review_candidate(
        &self,
        actor: &Actor,
        artist: &str,
        request: ConfirmArtistReviewCandidate,
    ) -> Result<ArtistReviewConfirmation> {
        let candidate = request.provider_artist_id.as_str();
        if request.request_id.is_nil() {
            return Err(Error::Invalid("Use a nonempty request".into()));
        }
        if artist.is_empty()
            || artist.len() > MAX_ID_LENGTH
            || candidate.is_empty()
            || candidate.len() > MAX_ID_LENGTH
        {
            return Err(Error::NotFound);
        }
        let digest = receipts::fingerprint("hito-artist-review-confirm:v1", artist, &request)?;
        let mut tx = self.store.pool.begin_with("BEGIN IMMEDIATE").await?;
        actor.check_session_on(&mut tx).await?;
        credit_name(&mut tx, actor, artist).await?;
        if let Some(receipt) = receipts::replay(&mut tx, actor, request.request_id, &digest).await?
        {
            return Ok(receipt);
        }
        let settings = policy::read(&mut tx, actor).await?;
        if !policy::selected(&settings, &MetadataProvider::BuiltinMusicBrainz) {
            return Err(policy::changed());
        }
        let stored: bool = sqlx::query_scalar(&format!(
            "SELECT EXISTS(SELECT 1 FROM artist_name_candidates c
             WHERE c.user_id=? AND c.artist_id=? AND c.provider_artist_id=? AND {OFFERED})"
        ))
        .bind(actor.id())
        .bind(artist)
        .bind(candidate)
        .fetch_one(&mut *tx)
        .await?;
        let chosen = undismissed_offer(&mut tx, actor, artist, candidate).await?
            && sqlx::query_scalar::<_, bool>(&format!(
                "SELECT EXISTS({})",
                edited("AND rc.artist_id=?2")
            ))
            .bind(actor.id())
            .bind(artist)
            .fetch_one(&mut *tx)
            .await?;
        let matched = !stored
            && matched_offers(&mut tx, actor, artist)
                .await?
                .iter()
                .any(|offer| offer.provider_artist_id == candidate)
            && !dismissed_ids(&mut tx, actor, artist)
                .await?
                .contains(candidate)
            && sqlx::query_scalar::<_, bool>(&format!(
                "SELECT EXISTS({})",
                undecided("AND rc.artist_id=?2")
            ))
            .bind(actor.id())
            .bind(artist)
            .fetch_one(&mut *tx)
            .await?;
        let searched = stored || matched;
        if !searched && !chosen {
            return Err(Error::NotFound);
        }
        let after = request.after.as_deref().unwrap_or_default();
        if after.len() > MAX_ID_LENGTH {
            return Err(Error::NotFound);
        }
        let credit_only = "AND rc.artist_id=?2";
        let from_cursor = "AND rc.artist_id=?2 AND r.id>?4";
        let ahead: i64 = sqlx::query_scalar(&format!(
            "SELECT count(*) FROM ({})",
            tracks_of(searched, chosen, from_cursor)
        ))
        .bind(actor.id())
        .bind(artist)
        .bind(0_i64)
        .bind(after)
        .fetch_one(&mut *tx)
        .await?;
        let targets: Vec<String> = sqlx::query_scalar(&format!(
            "SELECT id FROM ({}) ORDER BY id LIMIT ?3",
            tracks_of(searched, chosen, from_cursor)
        ))
        .bind(actor.id())
        .bind(artist)
        .bind(MAX_APPLIED)
        .bind(after)
        .fetch_all(&mut *tx)
        .await?;
        let mut pending = Vec::new();
        let mut kept = 0;
        for recording in &targets {
            match plan_for_credit(
                &mut tx,
                actor,
                recording,
                candidate,
                settings.revision,
                EvidenceKind::UserConfirmedCandidate,
                Some(artist),
            )
            .await?
            {
                Planned::Match(item) => pending.push(*item),
                Planned::Declined { .. } => kept += 1,
            }
        }
        let remaining = ahead - targets.len() as i64;
        let next_after = targets.last().filter(|_| remaining > 0).cloned();
        let receipt = ArtistReviewConfirmation {
            request_id: request.request_id,
            artist_id: artist.into(),
            provider_artist_id: candidate.into(),
            applied: pending.len() as i64,
            kept,
            remaining,
            next_after,
        };
        let order = receipts::save(&mut tx, actor, &digest, request.request_id, &receipt).await?;
        commit(&mut tx, actor, &pending, order).await?;
        if remaining == 0 {
            if searched {
                let left: i64 = sqlx::query_scalar(&format!(
                    "SELECT count(*) FROM ({})",
                    undecided(credit_only)
                ))
                .bind(actor.id())
                .bind(artist)
                .fetch_one(&mut *tx)
                .await?;
                if left > 0 {
                    sqlx::query(
                        "INSERT OR IGNORE INTO artist_review_dismissals(user_id,artist_id,provider_artist_id)
                         SELECT user_id,artist_id,provider_artist_id FROM artist_name_candidates
                         WHERE user_id=? AND artist_id=?",
                    )
                    .bind(actor.id())
                    .bind(artist)
                    .execute(&mut *tx)
                    .await?;
                    if matched {
                        dismiss_one(&mut tx, actor, artist, candidate).await?;
                    }
                }
            }
            if chosen {
                let left: i64 =
                    sqlx::query_scalar(&format!("SELECT count(*) FROM ({})", edited(credit_only)))
                        .bind(actor.id())
                        .bind(artist)
                        .fetch_one(&mut *tx)
                        .await?;
                if left > 0 {
                    dismiss_one(&mut tx, actor, artist, candidate).await?;
                }
            }
        }
        actor.check_session_on(&mut tx).await?;
        tx.commit().await?;
        Ok(receipt)
    }

    #[tracing::instrument(level = "info", skip_all, fields(operation = "artist_review_dismiss", request_id = %Uuid::new_v4()))]
    pub async fn dismiss_artist_review_candidates(
        &self,
        actor: &Actor,
        artist: &str,
        request: DismissArtistReviewCandidates,
    ) -> Result<ArtistReviewDismissal> {
        let started = Instant::now();
        let result = self.record_review_dismissal(actor, artist, request).await;
        tracing::info!(
            outcome = result.as_ref().map_or_else(|e| e.code(), |_| "dismissed"),
            dismissed = result.as_ref().map_or(0, |dismissal| dismissal.dismissed),
            elapsed_ms = started.elapsed().as_millis() as u64,
            "Artist review candidates dismissed"
        );
        result
    }

    async fn record_review_dismissal(
        &self,
        actor: &Actor,
        artist: &str,
        request: DismissArtistReviewCandidates,
    ) -> Result<ArtistReviewDismissal> {
        if artist.is_empty()
            || artist.len() > MAX_ID_LENGTH
            || request
                .provider_artist_id
                .as_ref()
                .is_some_and(|id| id.is_empty() || id.len() > MAX_ID_LENGTH)
        {
            return Err(Error::NotFound);
        }
        let mut tx = self.store.pool.begin_with("BEGIN IMMEDIATE").await?;
        actor.check_session_on(&mut tx).await?;
        credit_name(&mut tx, actor, artist).await?;
        let offers = offered_ids(&mut tx, actor, artist).await?;
        let dismissed = if let Some(candidate) = &request.provider_artist_id {
            let known: bool = sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM artist_name_candidates WHERE user_id=? AND artist_id=? AND provider_artist_id=?)",
            )
            .bind(actor.id())
            .bind(artist)
            .bind(candidate)
            .fetch_one(&mut *tx)
            .await?;
            if !known && !offers.contains(candidate) {
                return Err(Error::NotFound);
            }
            dismiss_one(&mut tx, actor, artist, candidate).await?
        } else {
            let mut dismissed = sqlx::query(
                "INSERT OR IGNORE INTO artist_review_dismissals(user_id,artist_id,provider_artist_id)
                 SELECT user_id,artist_id,provider_artist_id FROM artist_name_candidates
                 WHERE user_id=? AND artist_id=?",
            )
            .bind(actor.id())
            .bind(artist)
            .execute(&mut *tx)
            .await?
            .rows_affected();
            for offer in &offers {
                dismissed += dismiss_one(&mut tx, actor, artist, offer).await?;
            }
            dismissed
        };
        tx.commit().await?;
        Ok(ArtistReviewDismissal {
            artist_id: artist.into(),
            dismissed: dismissed as i64,
        })
    }

    #[tracing::instrument(level = "info", skip_all, fields(operation = "artist_review_restore", request_id = %Uuid::new_v4()))]
    pub async fn restore_artist_review_candidates(
        &self,
        actor: &Actor,
        artist: &str,
        request: RestoreArtistReviewCandidates,
    ) -> Result<ArtistReviewRestoration> {
        let started = Instant::now();
        let result = self.remove_review_dismissals(actor, artist, request).await;
        tracing::info!(
            outcome = result.as_ref().map_or_else(|e| e.code(), |_| "restored"),
            restored = result
                .as_ref()
                .map_or(0, |restoration| restoration.restored),
            elapsed_ms = started.elapsed().as_millis() as u64,
            "Artist review dismissals restored"
        );
        result
    }

    async fn remove_review_dismissals(
        &self,
        actor: &Actor,
        artist: &str,
        request: RestoreArtistReviewCandidates,
    ) -> Result<ArtistReviewRestoration> {
        if artist.is_empty()
            || artist.len() > MAX_ID_LENGTH
            || request
                .provider_artist_id
                .as_ref()
                .is_some_and(|id| id.is_empty() || id.len() > MAX_ID_LENGTH)
        {
            return Err(Error::NotFound);
        }
        let mut tx = self.store.pool.begin_with("BEGIN IMMEDIATE").await?;
        actor.check_session_on(&mut tx).await?;
        credit_name(&mut tx, actor, artist).await?;
        let restored = sqlx::query(
            "DELETE FROM artist_review_dismissals WHERE user_id=?1 AND artist_id=?2
             AND (?3 IS NULL OR provider_artist_id=?3)",
        )
        .bind(actor.id())
        .bind(artist)
        .bind(&request.provider_artist_id)
        .execute(&mut *tx)
        .await?
        .rows_affected();
        tx.commit().await?;
        Ok(ArtistReviewRestoration {
            artist_id: artist.into(),
            restored: restored as i64,
        })
    }
}
