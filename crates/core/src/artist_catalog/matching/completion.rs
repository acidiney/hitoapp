use super::{
    apply::{self, AUTOMATIC_CHOICE, EvidenceKind, Planned, REPLACEABLE, plan_for_credit},
    credit::names_several,
    settle::empty_receipt,
};
use crate::artist_catalog::{
    MetadataProvider, automation,
    identity::{groups::current_choices, receipts},
    policy,
};
use crate::{Actor, ArtistCatalog, Result};
use std::time::Instant;
use uuid::Uuid;

const MAX_CREDITS: i64 = 100;
const MAX_COMPLETED: usize = 1000;

#[derive(sqlx::FromRow)]
struct Decided {
    credit_id: String,
    name: String,
    provider_artist_id: String,
    source_tracks: i64,
}

fn undecided(credit: &str) -> String {
    format!(
        "SELECT r.id FROM account_recording_credits rc
         JOIN recordings r ON r.id=rc.recording_id
         JOIN library_members m ON m.library_id=r.library_id AND m.user_id=rc.user_id
         LEFT JOIN personal_recording_metadata p ON p.recording_id=r.id AND p.user_id=rc.user_id
         WHERE rc.user_id=?1 AND rc.artist_id={credit} AND r.metadata_observation_id IS NOT NULL
            AND p.artist_operation_id IS NULL
            AND NOT EXISTS (SELECT 1 FROM recording_artist_identity_choices c
                WHERE c.user_id=rc.user_id AND c.recording_id=r.id AND NOT {REPLACEABLE})"
    )
}

const ONE_CREDIT: &str =
    " AND choice.recording_id IN (SELECT scoped.recording_id FROM account_recording_credits scoped
    WHERE scoped.user_id=?1 AND scoped.artist_id=?3)";

fn decided(scope: &str) -> String {
    format!(
        "WITH current AS (
            SELECT choice.artist_id AS credit_id,choice.group_id AS group_id,NOT {AUTOMATIC_CHOICE} AS own
            {} ?1{scope}),
        decided AS (
            SELECT credit_id,min(group_id) AS group_id,sum(own) AS source_tracks FROM current
            GROUP BY credit_id HAVING count(DISTINCT group_id)=1 AND sum(own)>0)
        SELECT d.credit_id,a.name,g.provider_artist_id,d.source_tracks FROM decided d
        JOIN account_artists a ON a.user_id=?1 AND a.id=d.credit_id
        JOIN account_artist_identity_groups g ON g.user_id=?1 AND g.id=d.group_id
            AND g.provider_kind='builtin_musicbrainz'
        WHERE NOT EXISTS (SELECT 1 FROM artist_review_dismissals dismissed WHERE dismissed.user_id=?1
                AND dismissed.artist_id=d.credit_id AND dismissed.provider_artist_id=g.provider_artist_id)
            AND NOT EXISTS (SELECT 1 FROM artist_match_dismissals dismissed WHERE dismissed.user_id=?1
                AND dismissed.artist_id=d.credit_id AND dismissed.group_id=d.group_id)
            AND EXISTS ({})
        ORDER BY d.credit_id LIMIT ?2",
        current_choices(),
        undecided("d.credit_id")
    )
}

impl ArtistCatalog {
    pub(in crate::artist_catalog) async fn complete_decided_credits(
        &self,
        actor: &Actor,
    ) -> Result<i64> {
        self.complete_credits(actor, None).await
    }

    pub(in crate::artist_catalog) async fn complete_after_decision(
        &self,
        actor: &Actor,
        credit: &str,
    ) {
        if let Err(error) = self.complete_credits(actor, Some(credit)).await {
            tracing::warn!(
                event = "credit_completion_failed",
                error_kind = error.code(),
                "The rest of the credit could not be completed"
            );
        }
    }

    async fn complete_credits(&self, actor: &Actor, only: Option<&str>) -> Result<i64> {
        let started = Instant::now();
        let candidates: Vec<Decided> = {
            let mut reading = self.store.pool.begin().await?;
            actor.check_session_on(&mut reading).await?;
            let found = match only {
                None => {
                    sqlx::query_as(&decided(""))
                        .bind(actor.id())
                        .bind(MAX_CREDITS)
                        .fetch_all(&mut *reading)
                        .await?
                }
                Some(credit) => {
                    sqlx::query_as(&decided(ONE_CREDIT))
                        .bind(actor.id())
                        .bind(1_i64)
                        .bind(credit)
                        .fetch_all(&mut *reading)
                        .await?
                }
            };
            reading.commit().await?;
            found
        };
        if candidates.is_empty() {
            return Ok(0);
        }
        let mut tx = self.store.pool.begin_with("BEGIN IMMEDIATE").await?;
        actor.check_session_on(&mut tx).await?;
        let settings = policy::read(&mut tx, actor).await?;
        if !policy::selected(&settings, &MetadataProvider::BuiltinMusicBrainz) {
            return Ok(0);
        }
        let mut capped = candidates.len() as i64 == MAX_CREDITS;
        let mut pending = Vec::new();
        for candidate in &candidates {
            let credit: Option<Decided> = sqlx::query_as(&decided(ONE_CREDIT))
                .bind(actor.id())
                .bind(1_i64)
                .bind(&candidate.credit_id)
                .fetch_optional(&mut *tx)
                .await?;
            let Some(credit) = credit else {
                continue;
            };
            if names_several(&credit.name) {
                continue;
            }
            let room = MAX_COMPLETED - pending.len();
            let mut tracks: Vec<String> =
                sqlx::query_scalar(&format!("{} ORDER BY r.id LIMIT ?3", undecided("?2")))
                    .bind(actor.id())
                    .bind(&credit.credit_id)
                    .bind(room as i64 + 1)
                    .fetch_all(&mut *tx)
                    .await?;
            if tracks.len() > room {
                capped = true;
                tracks.truncate(room);
            }
            let kind =
                EvidenceKind::Completion(u32::try_from(credit.source_tracks).unwrap_or(u32::MAX));
            for recording in &tracks {
                if let Planned::Match(item) = plan_for_credit(
                    &mut tx,
                    actor,
                    recording,
                    &credit.provider_artist_id,
                    settings.revision,
                    kind,
                    Some(&credit.credit_id),
                )
                .await?
                {
                    pending.push(*item);
                }
            }
            if pending.len() >= MAX_COMPLETED {
                capped = true;
                break;
            }
        }
        let completed = pending.len() as i64;
        if !pending.is_empty() {
            let request_id = Uuid::new_v4();
            let digest =
                receipts::fingerprint("hito-artist-credit-completion:v1", "", &request_id)?;
            let mut receipt = empty_receipt(request_id, None);
            receipt.matched = completed;
            let order = receipts::save(&mut tx, actor, &digest, request_id, &receipt).await?;
            apply::commit(&mut tx, actor, &pending, order).await?;
            if capped {
                automation::request_for_account(&mut tx, actor.id()).await?;
            }
        }
        actor.check_session_on(&mut tx).await?;
        tx.commit().await?;
        tracing::info!(
            event = "credit_completion",
            credits = candidates.len(),
            completed,
            capped,
            elapsed_ms = started.elapsed().as_millis() as u64,
            "Decided credits completed"
        );
        Ok(completed)
    }
}
