//! The write half shared by the provider passes: every examined recording is re-validated in one
//! immediate transaction, settled answers are remembered, and matches are applied with a receipt.
use super::{
    ArtistProviderMatchReceipt,
    apply::{self, EvidenceKind, Planned, plan},
};
use crate::artist_catalog::{MetadataProvider, identity::receipts, policy};
use crate::{Actor, ArtistCatalog, Result};
use uuid::Uuid;

/// What the provider evidence of one recording concludes.
pub(super) enum Outcome {
    Artist(String, EvidenceKind),
    Uncertain,
    Unavailable,
}

/// A recording the pass asked about, with the observation and personal artist operation it saw.
pub(super) struct Examined {
    pub recording: String,
    pub observation: String,
    pub operation: Option<String>,
    pub outcome: Outcome,
}

/// Where settled uncertain answers are remembered, per pass.
#[derive(Clone, Copy)]
pub(super) enum Checks {
    Evidence,
    Name,
}

impl Checks {
    fn table(self) -> &'static str {
        match self {
            Self::Evidence => "artist_provider_checks",
            Self::Name => "artist_name_checks",
        }
    }
}

pub(super) struct Settlement<'a> {
    pub request_id: Uuid,
    pub digest: &'a str,
    pub next_after: Option<String>,
    pub checks: Checks,
}

pub(super) fn empty_receipt(
    request_id: Uuid,
    next_after: Option<String>,
) -> ArtistProviderMatchReceipt {
    ArtistProviderMatchReceipt {
        request_id,
        matched: 0,
        uncertain: 0,
        declined: 0,
        unavailable: 0,
        next_after,
    }
}

impl ArtistCatalog {
    pub(super) async fn settle(
        &self,
        actor: &Actor,
        settlement: Settlement<'_>,
        examined: Vec<Examined>,
    ) -> Result<ArtistProviderMatchReceipt> {
        let Settlement {
            request_id,
            digest,
            next_after,
            checks,
        } = settlement;
        let mut tx = self.store.pool.begin_with("BEGIN IMMEDIATE").await?;
        actor.check_session_on(&mut tx).await?;
        if let Some(receipt) = receipts::replay(&mut tx, actor, request_id, digest).await? {
            return Ok(receipt);
        }
        let settings = policy::read(&mut tx, actor).await?;
        if !policy::selected(&settings, &MetadataProvider::BuiltinMusicBrainz) {
            return Ok(empty_receipt(request_id, None));
        }
        let (mut uncertain, mut declined, mut unavailable) = (0, 0, 0);
        let mut pending = Vec::new();
        for item in &examined {
            let remember = match &item.outcome {
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
                    .bind(&item.recording)
                    .fetch_optional(&mut *tx)
                    .await?
                    .flatten();
                    if current.as_deref() != Some(item.observation.as_str()) {
                        declined += 1;
                        false
                    } else {
                        match plan(
                            &mut tx,
                            actor,
                            &item.recording,
                            artist,
                            settings.revision,
                            *kind,
                        )
                        .await?
                        {
                            Planned::Match(pending_match) => {
                                pending.push(*pending_match);
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
                sqlx::query(&format!(
                    "INSERT INTO {}(user_id,recording_id,observation_id,artist_operation_id) VALUES(?,?,?,?)
                     ON CONFLICT(user_id,recording_id) DO UPDATE SET observation_id=excluded.observation_id,artist_operation_id=excluded.artist_operation_id",
                    checks.table()
                ))
                .bind(actor.id())
                .bind(&item.recording)
                .bind(&item.observation)
                .bind(&item.operation)
                .execute(&mut *tx)
                .await?;
            }
        }
        let mut receipt = empty_receipt(request_id, next_after);
        receipt.matched = pending.len() as i64;
        receipt.uncertain = uncertain;
        receipt.declined = declined;
        receipt.unavailable = unavailable;
        if !pending.is_empty() {
            let order = receipts::save(&mut tx, actor, digest, request_id, &receipt).await?;
            apply::commit(&mut tx, actor, &pending, order).await?;
        }
        actor.check_session_on(&mut tx).await?;
        tx.commit().await?;
        Ok(receipt)
    }
}
