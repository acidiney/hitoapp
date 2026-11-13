use super::{
    MAX_ASSOCIATIONS,
    associations::{self, LINKED},
    shared_corrections::{SharedCorrectionRef, applied_for_association},
};
use crate::{Actor, ArtistCatalog, Error, Result, identity::recording_access};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::SqliteConnection;
use std::time::Instant;
use utoipa::ToSchema;
use uuid::Uuid;

#[derive(Clone, Deserialize, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct UnlinkRecordingArtist {
    pub request_id: Uuid,
    pub expected_revision: i64,
    pub association_id: Uuid,
}

#[derive(Clone, Deserialize, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct UndoRecordingArtistChange {
    pub request_id: Uuid,
    pub expected_revision: i64,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize, ToSchema)]
pub struct ArtistAssociationChangeReceipt {
    pub request_id: Uuid,
    pub recording_id: String,
    pub revision: i64,
    pub association_id: String,
    pub linked: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, ToSchema)]
pub struct ArtistAssociationUndo {
    pub association_id: String,
    pub linked: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ArtistCorrectionRetirement {
    CreditChanged,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, ToSchema)]
pub struct ArtistAssociationRetirement {
    pub association_id: String,
    pub reason: ArtistCorrectionRetirement,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, ToSchema)]
pub struct ArtistAssociationChanges {
    pub recording_id: String,
    pub revision: i64,
    pub undo: Option<ArtistAssociationUndo>,
    pub retired: Vec<ArtistAssociationRetirement>,
    pub shared_hidden: i64,
    pub shareable: bool,
    pub shared: Option<SharedCorrectionRef>,
}

const MAX_CHANGES: i64 = 1024;

struct Latest {
    revision: i64,
    association_id: String,
    linked: bool,
}

fn fingerprint(command: &str, recording: &str, request: &impl Serialize) -> Result<String> {
    let encoded = serde_json::to_vec(&(command, recording, request))
        .map_err(|_| Error::Internal("Artist association command could not be encoded".into()))?;
    Ok(format!("{:x}", Sha256::digest(encoded)))
}

fn changed() -> Error {
    Error::Conflict("The saved artist reviews changed. Review their current revision".into())
}

impl ArtistCatalog {
    #[tracing::instrument(level = "debug", skip_all, fields(operation = "artist_association_changes_read", request_id = %Uuid::new_v4()))]
    pub async fn recording_artist_association_changes(
        &self,
        actor: &Actor,
        recording: &str,
    ) -> Result<ArtistAssociationChanges> {
        let mut tx = self.store.pool.begin().await?;
        actor.check_session_on(&mut tx).await?;
        recording_access(&mut tx, actor, recording).await?;
        let latest = latest(&mut tx, actor, recording).await?;
        let retired: Vec<(String,)> = sqlx::query_as(
            "SELECT retirement.association_id FROM artist_association_change_retirements retirement
             JOIN recording_artist_associations assoc ON assoc.id=retirement.association_id
             WHERE retirement.user_id=? AND assoc.recording_id=? ORDER BY assoc.created_at,assoc.id LIMIT 128",
        )
        .bind(actor.id())
        .bind(recording)
        .fetch_all(&mut *tx)
        .await?;
        let (shareable, shared) = match &latest {
            Some(latest) if !latest.linked && actor.require_admin().is_ok() => {
                let builtin: bool = sqlx::query_scalar(
                    "SELECT provider_kind='builtin_musicbrainz' AND credit_target='whole_credit' FROM recording_artist_associations WHERE id=?",
                )
                .bind(&latest.association_id)
                .fetch_one(&mut *tx)
                .await?;
                let shared = applied_for_association(&mut tx, &latest.association_id).await?;
                (builtin && shared.is_none(), shared)
            }
            _ => (false, None),
        };
        let shared_hidden: i64 = sqlx::query_scalar(&format!(
            "SELECT count(*) FROM recording_artist_associations assoc
             WHERE assoc.user_id=? AND assoc.recording_id=?
                AND COALESCE((SELECT change.linked FROM recording_artist_association_changes change
                    WHERE change.user_id=assoc.user_id AND change.association_id=assoc.id
                    ORDER BY change.revision DESC LIMIT 1),1)=1
                AND NOT ({LINKED})"
        ))
        .bind(actor.id())
        .bind(recording)
        .fetch_one(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(ArtistAssociationChanges {
            shared_hidden,
            shareable,
            shared,
            retired: retired
                .into_iter()
                .map(|(association_id,)| ArtistAssociationRetirement {
                    association_id,
                    reason: ArtistCorrectionRetirement::CreditChanged,
                })
                .collect(),
            recording_id: recording.into(),
            revision: latest.as_ref().map_or(0, |latest| latest.revision),
            undo: latest.map(|latest| ArtistAssociationUndo {
                association_id: latest.association_id,
                linked: !latest.linked,
            }),
        })
    }

    #[tracing::instrument(level = "info", skip_all, fields(operation = "artist_association_unlink", request_id = %request.request_id))]
    pub async fn unlink_recording_artist(
        &self,
        actor: &Actor,
        recording: &str,
        request: UnlinkRecordingArtist,
    ) -> Result<ArtistAssociationChangeReceipt> {
        let started = Instant::now();
        let result = self.accept_unlink(actor, recording, request).await;
        finished("Artist association unlink finished", &result, started);
        result
    }

    #[tracing::instrument(level = "info", skip_all, fields(operation = "artist_association_undo", request_id = %request.request_id))]
    pub async fn undo_recording_artist_change(
        &self,
        actor: &Actor,
        recording: &str,
        request: UndoRecordingArtistChange,
    ) -> Result<ArtistAssociationChangeReceipt> {
        let started = Instant::now();
        let result = self.accept_undo(actor, recording, request).await;
        finished("Artist association undo finished", &result, started);
        result
    }

    async fn accept_unlink(
        &self,
        actor: &Actor,
        recording: &str,
        request: UnlinkRecordingArtist,
    ) -> Result<ArtistAssociationChangeReceipt> {
        if request.request_id.is_nil()
            || request.association_id.is_nil()
            || request.expected_revision < 0
        {
            return Err(Error::Invalid(
                "Use a nonempty request and association with a nonnegative revision".into(),
            ));
        }
        let digest = fingerprint("hito-artist-association-unlink:v1", recording, &request)?;
        let mut tx = self.store.pool.begin_with("BEGIN IMMEDIATE").await?;
        actor.check_session_on(&mut tx).await?;
        recording_access(&mut tx, actor, recording).await?;
        if let Some(receipt) = replay(&mut tx, actor, request.request_id, &digest).await? {
            return Ok(receipt);
        }
        let latest = latest(&mut tx, actor, recording).await?;
        if latest.as_ref().map_or(0, |latest| latest.revision) != request.expected_revision {
            return Err(changed());
        }
        let association = associations::load(
            &mut tx,
            actor,
            recording,
            &request.association_id.to_string(),
        )
        .await?;
        let receipt = apply(
            &mut tx,
            actor,
            recording,
            request.request_id,
            &association.id,
            false,
        )
        .await?;
        save(&mut tx, actor, &digest, &receipt).await?;
        actor.check_session_on(&mut tx).await?;
        tx.commit().await?;
        Ok(receipt)
    }

    async fn accept_undo(
        &self,
        actor: &Actor,
        recording: &str,
        request: UndoRecordingArtistChange,
    ) -> Result<ArtistAssociationChangeReceipt> {
        if request.request_id.is_nil() || request.expected_revision < 0 {
            return Err(Error::Invalid(
                "Use a nonempty request and a nonnegative revision".into(),
            ));
        }
        let digest = fingerprint("hito-artist-association-undo:v1", recording, &request)?;
        let mut tx = self.store.pool.begin_with("BEGIN IMMEDIATE").await?;
        actor.check_session_on(&mut tx).await?;
        recording_access(&mut tx, actor, recording).await?;
        if let Some(receipt) = replay(&mut tx, actor, request.request_id, &digest).await? {
            return Ok(receipt);
        }
        let latest = latest(&mut tx, actor, recording).await?;
        if latest.as_ref().map_or(0, |latest| latest.revision) != request.expected_revision {
            return Err(changed());
        }
        let latest = latest.ok_or_else(|| {
            Error::Conflict("There is no saved artist review change to undo".into())
        })?;
        if !latest.linked {
            if applied_for_association(&mut tx, &latest.association_id)
                .await?
                .is_some()
            {
                return Err(Error::Conflict(
                    "Withdraw the shared correction made from this unlink before undoing it".into(),
                ));
            }
            ensure_room(&mut tx, actor, recording).await?;
        }
        let receipt = apply(
            &mut tx,
            actor,
            recording,
            request.request_id,
            &latest.association_id,
            !latest.linked,
        )
        .await?;
        save(&mut tx, actor, &digest, &receipt).await?;
        actor.check_session_on(&mut tx).await?;
        tx.commit().await?;
        Ok(receipt)
    }
}

fn finished(message: &str, result: &Result<ArtistAssociationChangeReceipt>, started: Instant) {
    tracing::info!(
        outcome = result.as_ref().map_or_else(|e| e.code(), |_| "accepted"),
        revision = result.as_ref().map_or(0, |receipt| receipt.revision),
        elapsed_ms = started.elapsed().as_millis() as u64,
        "{message}"
    );
}

async fn apply(
    connection: &mut SqliteConnection,
    actor: &Actor,
    recording: &str,
    request_id: Uuid,
    association: &str,
    linked: bool,
) -> Result<ArtistAssociationChangeReceipt> {
    let revision = append(connection, actor, recording, association, linked).await?;
    Ok(ArtistAssociationChangeReceipt {
        request_id,
        recording_id: recording.into(),
        revision,
        association_id: association.into(),
        linked,
    })
}

pub(super) async fn append(
    connection: &mut SqliteConnection,
    actor: &Actor,
    recording: &str,
    association: &str,
    linked: bool,
) -> Result<i64> {
    let revision: i64 = sqlx::query_scalar(
        "SELECT COALESCE(max(revision),0)+1 FROM recording_artist_association_changes
         WHERE user_id=? AND recording_id=?",
    )
    .bind(actor.id())
    .bind(recording)
    .fetch_one(&mut *connection)
    .await?;
    if revision > MAX_CHANGES {
        return Err(Error::Conflict(
            "This recording has too many saved artist review changes".into(),
        ));
    }
    sqlx::query(
        "INSERT INTO recording_artist_association_changes(user_id,recording_id,revision,association_id,linked)
         VALUES(?,?,?,?,?)",
    )
    .bind(actor.id())
    .bind(recording)
    .bind(revision)
    .bind(association)
    .bind(linked)
    .execute(&mut *connection)
    .await?;
    if !linked {
        sqlx::query(
            "UPDATE recording_artist_identity_choices SET retired=1,revision=revision+1
             WHERE user_id=? AND recording_id=? AND association_id=? AND retired=0",
        )
        .bind(actor.id())
        .bind(recording)
        .bind(association)
        .execute(&mut *connection)
        .await?;
        sqlx::query(
            "UPDATE recording_artist_member_identity_choices SET retired=1,revision=revision+1
             WHERE user_id=? AND recording_id=? AND association_id=? AND retired=0",
        )
        .bind(actor.id())
        .bind(recording)
        .bind(association)
        .execute(connection)
        .await?;
    }
    Ok(revision)
}

pub(super) async fn ensure_room(
    connection: &mut SqliteConnection,
    actor: &Actor,
    recording: &str,
) -> Result<()> {
    let linked: i64 = sqlx::query_scalar(&format!(
        "SELECT count(*) FROM recording_artist_associations assoc
         WHERE assoc.user_id=? AND assoc.recording_id=? AND {LINKED}"
    ))
    .bind(actor.id())
    .bind(recording)
    .fetch_one(connection)
    .await?;
    if linked >= MAX_ASSOCIATIONS {
        return Err(Error::Conflict(
            "This recording already has 32 linked artist associations".into(),
        ));
    }
    Ok(())
}

pub(super) async fn is_linked(
    connection: &mut SqliteConnection,
    association: &str,
) -> Result<bool> {
    Ok(sqlx::query_scalar(&format!(
        "SELECT {LINKED} FROM recording_artist_associations assoc WHERE assoc.id=?"
    ))
    .bind(association)
    .fetch_one(connection)
    .await?)
}

async fn latest(
    connection: &mut SqliteConnection,
    actor: &Actor,
    recording: &str,
) -> Result<Option<Latest>> {
    let row: Option<(i64, String, bool)> = sqlx::query_as(
        "SELECT revision,association_id,linked FROM recording_artist_association_changes
         WHERE user_id=? AND recording_id=? ORDER BY revision DESC LIMIT 1",
    )
    .bind(actor.id())
    .bind(recording)
    .fetch_optional(connection)
    .await?;
    Ok(row.map(|(revision, association_id, linked)| Latest {
        revision,
        association_id,
        linked,
    }))
}

async fn replay(
    connection: &mut SqliteConnection,
    actor: &Actor,
    request_id: Uuid,
    digest: &str,
) -> Result<Option<ArtistAssociationChangeReceipt>> {
    let previous: Option<(String, String)> = sqlx::query_as(
        "SELECT request_digest,receipt FROM recording_artist_association_receipts WHERE user_id=? AND request_id=?",
    )
    .bind(actor.id())
    .bind(request_id.to_string())
    .fetch_optional(connection)
    .await?;
    let Some((previous, receipt)) = previous else {
        return Ok(None);
    };
    if previous != digest {
        return Err(Error::Conflict(
            "This artist association request was already used with different contents".into(),
        ));
    }
    tracing::debug!(
        outcome = "replayed",
        "Artist association acknowledgement replayed"
    );
    serde_json::from_str(&receipt)
        .map(Some)
        .map_err(|_| Error::Internal("Stored artist association receipt is invalid".into()))
}

async fn save(
    connection: &mut SqliteConnection,
    actor: &Actor,
    digest: &str,
    receipt: &ArtistAssociationChangeReceipt,
) -> Result<()> {
    let encoded = serde_json::to_string(receipt)
        .map_err(|_| Error::Internal("Artist association receipt could not be encoded".into()))?;
    sqlx::query("INSERT INTO recording_artist_association_receipts(user_id,request_id,request_digest,receipt) VALUES(?,?,?,?)")
        .bind(actor.id()).bind(receipt.request_id.to_string()).bind(digest).bind(encoded)
        .execute(connection).await?;
    Ok(())
}
