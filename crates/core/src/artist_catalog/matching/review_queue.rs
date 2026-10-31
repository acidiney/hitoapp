//! The review queue: credits the automatic walk could not identify, with the artists it found
//! under their name, so that the account decides in one step. The walk stores the candidates; reads
//! contact no provider.
use super::{
    ArtistReviewCandidate, ArtistReviewConfirmation, ArtistReviewDismissal, ArtistReviewItem,
    ArtistReviewQueueQuery, ConfirmArtistReviewCandidate, DismissArtistReviewCandidates,
    apply::{EvidenceKind, Planned, REPLACEABLE, commit, plan},
    name_discography::NAME_RULE,
    name_probe::{Assessment, Candidate, distinct_titles},
    suggestions::credit_name,
};
use crate::artist_catalog::{MetadataProvider, REQUEST_BUDGET, identity::receipts, policy};
use crate::{Actor, ArtistCatalog, Error, Page, Result};
use hito_metadata_musicbrainz::NameCandidate;
use sqlx::SqliteConnection;
use std::{collections::HashMap, time::Instant};
use uuid::Uuid;

const DEFAULT_LIMIT: i64 = 20;
const MAX_LIMIT: i64 = 50;
const MAX_OFFSET: i64 = 100_000;
const MAX_CANDIDATES: i64 = 3;
/// Undecided tracks one confirmation applies; asking again continues.
const MAX_APPLIED: i64 = 200;
const MAX_ID_LENGTH: usize = 64;
const MAX_TEXT_CHARS: usize = 200;

/// An exact-name artist of a credit, as the walk saw it.
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

/// The candidates of a credit in search order, with the distinct sample titles each lists.
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

/// Replaces the candidates kept for a credit. Dismissals are separate and stay.
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

/// Recordings the walk examined and left undecided, which the account has not decided about and
/// whose artist the account has not edited. `{credit}` limits them to one credit.
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

const OFFERED: &str = "NOT EXISTS (SELECT 1 FROM artist_review_dismissals d
    WHERE d.user_id=c.user_id AND d.artist_id=c.artist_id AND d.provider_artist_id=c.provider_artist_id)";

fn queued() -> String {
    format!(
        "WITH undecided AS (SELECT artist_id,count(*) AS tracks FROM ({}) GROUP BY artist_id)
         SELECT a.id,a.name,u.tracks FROM undecided u
         JOIN account_artists a ON a.id=u.artist_id AND a.user_id=?1
         JOIN library_members m ON m.library_id=a.library_id AND m.user_id=a.user_id
         WHERE EXISTS (SELECT 1 FROM artist_name_candidates c
            WHERE c.user_id=a.user_id AND c.artist_id=a.id AND {OFFERED})",
        undecided("")
    )
}

impl ArtistCatalog {
    /// The credits the walk left undecided that have candidates, the largest credit first.
    #[tracing::instrument(level = "debug", skip_all, fields(operation = "artist_review_queue", request_id = %Uuid::new_v4()))]
    pub async fn artist_review_queue(
        &self,
        actor: &Actor,
        query: ArtistReviewQueueQuery,
    ) -> Result<Page<ArtistReviewItem>> {
        let started = Instant::now();
        let result = tokio::time::timeout(REQUEST_BUDGET, self.read_review_queue(actor, query))
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

    async fn read_review_queue(
        &self,
        actor: &Actor,
        query: ArtistReviewQueueQuery,
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
        let base = queued();
        let total: i64 = sqlx::query_scalar(&format!("SELECT count(*) FROM ({base})"))
            .bind(actor.id())
            .fetch_one(&mut *tx)
            .await?;
        let rows: Vec<(String, String, i64)> = sqlx::query_as(&format!(
            "{base} ORDER BY u.tracks DESC,a.name,a.id LIMIT ?2 OFFSET ?3"
        ))
        .bind(actor.id())
        .bind(limit)
        .bind(offset)
        .fetch_all(&mut *tx)
        .await?;
        let mut items = Vec::with_capacity(rows.len());
        for (artist_id, name, undecided_tracks) in rows {
            let candidates: Vec<ArtistReviewCandidate> = sqlx::query_as(&format!(
                "SELECT c.provider_artist_id,c.name,c.kind,c.disambiguation,c.country,
                        c.corroborated_titles,
                        EXISTS (SELECT 1 FROM account_artist_identity_groups g WHERE g.user_id=c.user_id
                            AND g.provider_kind='builtin_musicbrainz' AND g.provider_artist_id=c.provider_artist_id) AS in_library
                 FROM artist_name_candidates c WHERE c.user_id=? AND c.artist_id=? AND {OFFERED}
                 ORDER BY in_library DESC,c.corroborated_titles DESC,c.position LIMIT ?"
            ))
            .bind(actor.id())
            .bind(&artist_id)
            .bind(MAX_CANDIDATES)
            .fetch_all(&mut *tx)
            .await?;
            items.push(ArtistReviewItem {
                artist_id,
                name,
                undecided_tracks,
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

    /// Applies one candidate to the credit's undecided tracks. A track the account decided about,
    /// edited or unlinked keeps its state; the artist joins the account's chosen artists.
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
        let offered: bool = sqlx::query_scalar(&format!(
            "SELECT EXISTS(SELECT 1 FROM artist_name_candidates c
             WHERE c.user_id=? AND c.artist_id=? AND c.provider_artist_id=? AND {OFFERED})"
        ))
        .bind(actor.id())
        .bind(artist)
        .bind(candidate)
        .fetch_one(&mut *tx)
        .await?;
        if !offered {
            return Err(Error::NotFound);
        }
        let filter = "AND rc.artist_id=?2";
        let total: i64 =
            sqlx::query_scalar(&format!("SELECT count(*) FROM ({})", undecided(filter)))
                .bind(actor.id())
                .bind(artist)
                .fetch_one(&mut *tx)
                .await?;
        let targets: Vec<String> = sqlx::query_scalar(&format!(
            "SELECT id FROM ({}) ORDER BY id LIMIT ?3",
            undecided(filter)
        ))
        .bind(actor.id())
        .bind(artist)
        .bind(MAX_APPLIED)
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
        let remaining = total - targets.len() as i64;
        let receipt = ArtistReviewConfirmation {
            request_id: request.request_id,
            artist_id: artist.into(),
            provider_artist_id: candidate.into(),
            applied: pending.len() as i64,
            kept,
            remaining,
        };
        let order = receipts::save(&mut tx, actor, &digest, request.request_id, &receipt).await?;
        commit(&mut tx, actor, &pending, order).await?;
        if remaining == 0 && kept > 0 {
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
        actor.check_session_on(&mut tx).await?;
        tx.commit().await?;
        Ok(receipt)
    }

    /// "Not this artist" for one candidate, or "None of these" for every candidate the credit has.
    /// The credit leaves the queue once no candidate is left, and a later walk keeps the decision.
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
        let dismissed = if let Some(candidate) = &request.provider_artist_id {
            let known: bool = sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM artist_name_candidates WHERE user_id=? AND artist_id=? AND provider_artist_id=?)",
            )
            .bind(actor.id())
            .bind(artist)
            .bind(candidate)
            .fetch_one(&mut *tx)
            .await?;
            if !known {
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
            sqlx::query(
                "INSERT OR IGNORE INTO artist_review_dismissals(user_id,artist_id,provider_artist_id)
                 SELECT user_id,artist_id,provider_artist_id FROM artist_name_candidates
                 WHERE user_id=? AND artist_id=?",
            )
            .bind(actor.id())
            .bind(artist)
            .execute(&mut *tx)
            .await?
            .rows_affected()
        };
        tx.commit().await?;
        Ok(ArtistReviewDismissal {
            artist_id: artist.into(),
            dismissed: dismissed as i64,
        })
    }
}
