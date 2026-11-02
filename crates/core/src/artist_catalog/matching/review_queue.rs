use super::{
    ArtistReviewCandidate, ArtistReviewConfirmation, ArtistReviewDismissal, ArtistReviewItem,
    ArtistReviewOrigin, ArtistReviewQueueQuery, ArtistReviewRestoration,
    ConfirmArtistReviewCandidate, DismissArtistReviewCandidates, RestoreArtistReviewCandidates,
    apply::{EvidenceKind, Planned, REPLACEABLE, commit, plan},
    name_discography::NAME_RULE,
    name_probe::{Assessment, Candidate, distinct_titles},
    suggestions::credit_name,
};
use crate::artist_catalog::{
    MetadataProvider, REQUEST_BUDGET,
    identity::{groups::current_choices, receipts},
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
         JOIN recordings r ON r.id=p.recording_id AND r.metadata_observation_id IS NOT NULL
         JOIN account_recording_credits rc ON rc.recording_id=r.id AND rc.user_id=p.user_id
         JOIN library_members m ON m.library_id=r.library_id AND m.user_id=p.user_id
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

fn chosen_artist_match() -> String {
    format!(
        "SELECT 1 {} ?1 AND lower(trim(artist.name))=lower(trim(a.name))
            AND EXISTS (SELECT 1 FROM account_artist_identity_groups g
                WHERE g.user_id=choice.user_id AND g.id=choice.group_id AND g.provider_kind='builtin_musicbrainz'",
        current_choices()
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
enum Listing {
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

fn queued(listing: Listing) -> String {
    let filter = listing.filter();
    let stored = format!(
        "EXISTS (SELECT 1 FROM artist_name_candidates c
            WHERE c.user_id=a.user_id AND c.artist_id=a.id AND {filter})"
    );
    let unlisted = match listing {
        Listing::Offered => {
            "NOT EXISTS (SELECT 1 FROM artist_name_candidates c WHERE c.user_id=a.user_id AND c.artist_id=a.id)"
        }
        Listing::Dismissed => "0",
    };
    let dismissal = "EXISTS (SELECT 1 FROM artist_review_dismissals d WHERE d.user_id=a.user_id
        AND d.artist_id=a.id AND d.provider_artist_id=g.provider_artist_id)";
    let offer_dismissal = match listing {
        Listing::Offered => format!("NOT {dismissal}"),
        Listing::Dismissed => dismissal.to_owned(),
    };
    let chosen = format!("EXISTS ({}  AND {offer_dismissal}))", chosen_artist_match());
    format!(
        "WITH undecided AS (SELECT artist_id,count(*) AS tracks FROM ({}) GROUP BY artist_id),
              edited AS (SELECT artist_id,count(*) AS tracks FROM ({}) GROUP BY artist_id)
         SELECT * FROM (
            SELECT a.id AS id,a.name AS name,
                CASE WHEN u.tracks>0 AND ({stored} OR {unlisted}) THEN u.tracks ELSE 0 END
                + CASE WHEN e.tracks>0 AND {chosen} THEN e.tracks ELSE 0 END AS tracks,
                CASE WHEN u.tracks>0 AND {stored} THEN 1 ELSE 0 END AS stored,
                CASE WHEN e.tracks>0 AND {chosen} THEN 1 ELSE 0 END AS chosen
            FROM account_artists a
            JOIN library_members m ON m.library_id=a.library_id AND m.user_id=a.user_id
            LEFT JOIN undecided u ON u.artist_id=a.id
            LEFT JOIN edited e ON e.artist_id=a.id
            WHERE a.user_id=?1 AND (u.tracks>0 OR e.tracks>0))
         WHERE tracks>0",
        undecided(""),
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
        let rows: Vec<(String, String, i64, i64, i64, i64)> = sqlx::query_as(&format!(
            "SELECT id,name,tracks,stored,chosen,count(*) OVER () FROM ({base})
             ORDER BY (stored+chosen)=0,tracks DESC,name,id LIMIT ?2 OFFSET ?3"
        ))
        .bind(actor.id())
        .bind(limit)
        .bind(offset)
        .fetch_all(&mut *tx)
        .await?;
        let total = match rows.first() {
            Some(first) => first.5,
            None if offset == 0 => 0,
            None => {
                sqlx::query_scalar(&format!("SELECT count(*) FROM ({base})"))
                    .bind(actor.id())
                    .fetch_one(&mut *tx)
                    .await?
            }
        };
        let mut items = Vec::with_capacity(rows.len());
        for (artist_id, name, undecided_tracks, stored, chosen, _) in rows {
            let mut candidates = Vec::new();
            if chosen > 0 {
                let dismissed = dismissed_ids(&mut tx, actor, &artist_id).await?;
                for offer in chosen_offers(&mut tx, actor, &artist_id).await? {
                    let is_dismissed = dismissed.contains(&offer.provider_artist_id);
                    if is_dismissed == matches!(listing, Listing::Dismissed) {
                        candidates.push(ArtistReviewCandidate {
                            provider_artist_id: offer.provider_artist_id,
                            name: clipped(&offer.name, MAX_TEXT_CHARS),
                            kind: None,
                            disambiguation: None,
                            country: None,
                            corroborated_titles: 0,
                            in_library: true,
                            origin: ArtistReviewOrigin::ChosenArtist,
                        });
                    }
                }
            }
            if stored > 0 {
                let found: Vec<ArtistReviewCandidate> = sqlx::query_as(&format!(
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
                .await?;
                candidates.extend(found);
            }
            candidates.truncate(MAX_CANDIDATES as usize);
            let search_recording_id = if candidates.is_empty() {
                sqlx::query_scalar(&format!(
                    "SELECT min(id) FROM ({})",
                    undecided("AND rc.artist_id=?2")
                ))
                .bind(actor.id())
                .bind(&artist_id)
                .fetch_one(&mut *tx)
                .await?
            } else {
                None
            };
            items.push(ArtistReviewItem {
                artist_id,
                name,
                undecided_tracks,
                needs_search: candidates.is_empty(),
                search_recording_id,
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
        let searched: bool = sqlx::query_scalar(&format!(
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
            match plan(
                &mut tx,
                actor,
                recording,
                candidate,
                settings.revision,
                EvidenceKind::UserConfirmedCandidate,
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
                    sqlx::query(
                        "INSERT OR IGNORE INTO artist_review_dismissals(user_id,artist_id,provider_artist_id) VALUES(?,?,?)",
                    )
                    .bind(actor.id())
                    .bind(artist)
                    .bind(candidate)
                    .execute(&mut *tx)
                    .await?;
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
        let offers = chosen_offers(&mut tx, actor, artist).await?;
        let dismissed = if let Some(candidate) = &request.provider_artist_id {
            let known: bool = sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM artist_name_candidates WHERE user_id=? AND artist_id=? AND provider_artist_id=?)",
            )
            .bind(actor.id())
            .bind(artist)
            .bind(candidate)
            .fetch_one(&mut *tx)
            .await?;
            if !known
                && !offers
                    .iter()
                    .any(|offer| &offer.provider_artist_id == candidate)
            {
                return Err(Error::NotFound);
            }
            sqlx::query(
                "INSERT OR IGNORE INTO artist_review_dismissals(user_id,artist_id,provider_artist_id) VALUES(?,?,?)",
            )
            .bind(actor.id())
            .bind(artist)
            .bind(candidate)
            .execute(&mut *tx)
            .await?
            .rows_affected()
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
                dismissed += sqlx::query(
                    "INSERT OR IGNORE INTO artist_review_dismissals(user_id,artist_id,provider_artist_id) VALUES(?,?,?)",
                )
                .bind(actor.id())
                .bind(artist)
                .bind(&offer.provider_artist_id)
                .execute(&mut *tx)
                .await?
                .rows_affected();
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
