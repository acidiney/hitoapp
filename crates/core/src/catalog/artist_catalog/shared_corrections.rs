use super::identity::split_change;
use crate::{Actor, Catalog, Error, Result, identity::recording_access};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::SqliteConnection;
use std::time::Instant;
use utoipa::ToSchema;
use uuid::Uuid;

const MAX_LISTED: i64 = 100;

#[derive(Clone, Deserialize, Serialize, ToSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PromotionSource {
    /// The administrator's own unlink of one built-in artist review.
    Unlink {
        recording_id: String,
        association_id: Uuid,
    },
    /// The administrator's latest split of one built-in artist identity group.
    Split { group_id: String, revision: i64 },
}

#[derive(Clone, Deserialize, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct PromoteArtistCorrection {
    pub request_id: Uuid,
    pub expected_revision: i64,
    pub source: PromotionSource,
}

#[derive(Clone, Deserialize, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct UndoSharedArtistCorrection {
    pub request_id: Uuid,
    pub expected_revision: i64,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize, ToSchema)]
pub struct SharedArtistCorrectionReceipt {
    pub request_id: Uuid,
    pub correction_id: String,
    pub revision: i64,
    pub applied: bool,
    pub recordings: i64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum SharedCorrectionKind {
    Unlink,
    Split,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, ToSchema)]
pub struct SharedArtistCorrection {
    pub id: String,
    pub kind: SharedCorrectionKind,
    pub recordings: i64,
    /// Recordings whose tags changed after the correction was shared.
    pub retired: i64,
    pub applied: bool,
    pub revision: i64,
    pub promoted_by_me: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, ToSchema)]
pub struct SharedArtistCorrections {
    pub revision: i64,
    pub items: Vec<SharedArtistCorrection>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, ToSchema)]
pub struct SharedCorrectionRef {
    pub id: String,
    pub revision: i64,
}

/// The applied shared correction made from a split, if any.
pub(crate) async fn applied_for_change(
    connection: &mut SqliteConnection,
    change: &str,
) -> Result<Option<SharedCorrectionRef>> {
    applied(connection, "correction.source_change_id", change).await
}

/// The applied shared correction made from an unlink, if any.
pub(crate) async fn applied_for_association(
    connection: &mut SqliteConnection,
    association: &str,
) -> Result<Option<SharedCorrectionRef>> {
    applied(connection, "correction.source_association_id", association).await
}

async fn applied(
    connection: &mut SqliteConnection,
    column: &str,
    value: &str,
) -> Result<Option<SharedCorrectionRef>> {
    let row: Option<(String, i64)> = sqlx::query_as(&format!(
        "SELECT correction.id,state.revision FROM shared_artist_corrections correction
         JOIN shared_artist_correction_states state ON state.correction_seq=correction.seq
            AND state.revision=(SELECT max(revision) FROM shared_artist_correction_states WHERE correction_seq=correction.seq)
         WHERE {column}=? AND state.applied=1 ORDER BY correction.seq DESC LIMIT 1"
    ))
    .bind(value)
    .fetch_optional(connection)
    .await?;
    Ok(row.map(|(id, revision)| SharedCorrectionRef { id, revision }))
}

struct Subject {
    recording: String,
    observation: String,
    artist: String,
}

fn fingerprint(command: &str, scope: &str, request: &impl Serialize) -> Result<String> {
    let encoded = serde_json::to_vec(&(command, scope, request))
        .map_err(|_| Error::Internal("Artist correction command could not be encoded".into()))?;
    Ok(format!("{:x}", Sha256::digest(encoded)))
}

fn stale() -> Error {
    Error::Conflict("The shared artist corrections changed. Review their current revision".into())
}

fn builtin_only() -> Error {
    Error::Invalid("Only corrections to built-in MusicBrainz artists can be shared".into())
}

async fn revision(connection: &mut SqliteConnection) -> Result<i64> {
    Ok(
        sqlx::query_scalar("SELECT COALESCE(max(seq),0) FROM shared_artist_corrections")
            .fetch_one(connection)
            .await?,
    )
}

async fn replay(
    connection: &mut SqliteConnection,
    actor: &Actor,
    request_id: Uuid,
    digest: &str,
) -> Result<Option<SharedArtistCorrectionReceipt>> {
    let previous: Option<(String, String)> = sqlx::query_as(
        "SELECT request_digest,receipt FROM shared_artist_correction_receipts WHERE user_id=? AND request_id=?",
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
            "This shared artist correction request was already used with different contents".into(),
        ));
    }
    tracing::debug!(
        outcome = "replayed",
        "Shared artist correction acknowledgement replayed"
    );
    serde_json::from_str(&receipt)
        .map(Some)
        .map_err(|_| Error::Internal("Stored shared artist correction receipt is invalid".into()))
}

async fn save(
    connection: &mut SqliteConnection,
    actor: &Actor,
    digest: &str,
    receipt: &SharedArtistCorrectionReceipt,
) -> Result<()> {
    let encoded = serde_json::to_string(receipt).map_err(|_| {
        Error::Internal("Shared artist correction receipt could not be encoded".into())
    })?;
    sqlx::query("INSERT INTO shared_artist_correction_receipts(user_id,request_id,request_digest,receipt) VALUES(?,?,?,?)")
        .bind(actor.id()).bind(receipt.request_id.to_string()).bind(digest).bind(encoded)
        .execute(connection).await?;
    Ok(())
}

struct Source {
    kind: &'static str,
    change: Option<String>,
    association: Option<String>,
    subjects: Vec<Subject>,
}

async fn unlink_source(
    connection: &mut SqliteConnection,
    actor: &Actor,
    recording: &str,
    association: &str,
) -> Result<Source> {
    recording_access(connection, actor, recording).await?;
    let row: Option<(String, String, String, Option<bool>)> = sqlx::query_as(
        "SELECT assoc.provider_kind,assoc.observation_id,assoc.provider_artist_id,
            (SELECT change.linked FROM recording_artist_association_changes change
             WHERE change.user_id=assoc.user_id AND change.association_id=assoc.id ORDER BY change.revision DESC LIMIT 1)
         FROM recording_artist_associations assoc WHERE assoc.id=? AND assoc.user_id=? AND assoc.recording_id=?",
    )
    .bind(association)
    .bind(actor.id())
    .bind(recording)
    .fetch_optional(&mut *connection)
    .await?;
    let (kind, observation, artist, linked) = row.ok_or(Error::NotFound)?;
    if kind != "builtin_musicbrainz" {
        return Err(builtin_only());
    }
    if linked != Some(false) {
        return Err(Error::Conflict(
            "Unlink the saved artist review before sharing the unlink".into(),
        ));
    }
    Ok(Source {
        kind: "unlink",
        change: None,
        association: Some(association.into()),
        subjects: vec![Subject {
            recording: recording.into(),
            observation,
            artist,
        }],
    })
}

async fn split_source(
    connection: &mut SqliteConnection,
    actor: &Actor,
    group: &str,
    revision: i64,
) -> Result<Source> {
    let change = split_change(connection, actor, group).await?;
    if !change.builtin {
        return Err(builtin_only());
    }
    if change.revision != revision || change.kind != "split" {
        return Err(Error::Conflict(
            "Only the latest split of this artist can be shared".into(),
        ));
    }
    let subjects = change
        .observations
        .into_iter()
        .map(|(recording, observation)| Subject {
            recording,
            observation,
            artist: change.provider_artist_id.clone(),
        })
        .collect::<Vec<_>>();
    if subjects.is_empty() {
        return Err(Error::Conflict(
            "This split no longer applies to any recording".into(),
        ));
    }
    Ok(Source {
        kind: "split",
        change: Some(change.id),
        association: None,
        subjects,
    })
}

async fn already_shared(connection: &mut SqliteConnection, subjects: &[Subject]) -> Result<bool> {
    for subject in subjects {
        let shared: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM shared_artist_correction_subjects subject
             WHERE subject.retired_reason IS NULL AND subject.recording_id=? AND subject.observation_id=? AND subject.provider_artist_id=?
                AND (SELECT state.applied FROM shared_artist_correction_states state
                     WHERE state.correction_seq=subject.correction_seq ORDER BY state.revision DESC LIMIT 1)=1)",
        )
        .bind(&subject.recording)
        .bind(&subject.observation)
        .bind(&subject.artist)
        .fetch_one(&mut *connection)
        .await?;
        if !shared {
            return Ok(false);
        }
    }
    Ok(true)
}

impl Catalog {
    #[tracing::instrument(level = "debug", skip_all, fields(operation = "shared_artist_corrections_read", request_id = %Uuid::new_v4()))]
    pub async fn shared_artist_corrections(
        &self,
        actor: &Actor,
    ) -> Result<SharedArtistCorrections> {
        actor.require_admin()?;
        let mut tx = self.store.pool.begin().await?;
        actor.check_session_on(&mut tx).await?;
        let current = revision(&mut tx).await?;
        let rows: Vec<(String, String, String, i64, i64, i64, bool)> = sqlx::query_as(
            "SELECT correction.id,correction.source_kind,correction.promoted_by,
                (SELECT count(*) FROM shared_artist_correction_subjects s WHERE s.correction_seq=correction.seq),
                (SELECT count(*) FROM shared_artist_correction_subjects s WHERE s.correction_seq=correction.seq AND s.retired_reason IS NOT NULL),
                state.revision,state.applied
             FROM shared_artist_corrections correction
             JOIN shared_artist_correction_states state ON state.correction_seq=correction.seq
                AND state.revision=(SELECT max(revision) FROM shared_artist_correction_states WHERE correction_seq=correction.seq)
             ORDER BY correction.seq DESC LIMIT ?",
        )
        .bind(MAX_LISTED)
        .fetch_all(&mut *tx)
        .await?;
        tx.commit().await?;
        let items = rows
            .into_iter()
            .map(
                |(id, kind, by, recordings, retired, revision, applied)| SharedArtistCorrection {
                    id,
                    kind: if kind == "unlink" {
                        SharedCorrectionKind::Unlink
                    } else {
                        SharedCorrectionKind::Split
                    },
                    recordings,
                    retired,
                    applied,
                    revision,
                    promoted_by_me: by == actor.id(),
                },
            )
            .collect();
        Ok(SharedArtistCorrections {
            revision: current,
            items,
        })
    }

    #[tracing::instrument(level = "info", skip_all, fields(operation = "artist_correction_promote", request_id = %request.request_id))]
    pub async fn promote_artist_correction(
        &self,
        actor: &Actor,
        request: PromoteArtistCorrection,
    ) -> Result<SharedArtistCorrectionReceipt> {
        let started = Instant::now();
        let result = self.accept_promotion(actor, request).await;
        finished("Artist correction promotion finished", &result, started);
        result
    }

    #[tracing::instrument(level = "info", skip_all, fields(operation = "artist_correction_undo", request_id = %request.request_id))]
    pub async fn undo_shared_artist_correction(
        &self,
        actor: &Actor,
        correction: &str,
        request: UndoSharedArtistCorrection,
    ) -> Result<SharedArtistCorrectionReceipt> {
        let started = Instant::now();
        let result = self
            .accept_correction_undo(actor, correction, request)
            .await;
        finished("Artist correction undo finished", &result, started);
        result
    }

    async fn accept_promotion(
        &self,
        actor: &Actor,
        request: PromoteArtistCorrection,
    ) -> Result<SharedArtistCorrectionReceipt> {
        actor.require_admin()?;
        if request.request_id.is_nil() || request.expected_revision < 0 {
            return Err(Error::Invalid(
                "Use a nonempty request and a nonnegative revision".into(),
            ));
        }
        let digest = fingerprint("hito-artist-correction-promote:v1", "", &request)?;
        let mut tx = self.store.pool.begin_with("BEGIN IMMEDIATE").await?;
        actor.check_session_on(&mut tx).await?;
        if let Some(receipt) = replay(&mut tx, actor, request.request_id, &digest).await? {
            return Ok(receipt);
        }
        if revision(&mut tx).await? != request.expected_revision {
            return Err(stale());
        }
        let source = match &request.source {
            PromotionSource::Unlink {
                recording_id,
                association_id,
            } if !association_id.is_nil() => {
                unlink_source(&mut tx, actor, recording_id, &association_id.to_string()).await?
            }
            PromotionSource::Split { group_id, revision } => {
                split_source(&mut tx, actor, group_id, *revision).await?
            }
            _ => return Err(Error::Invalid("Use a nonempty association".into())),
        };
        if already_shared(&mut tx, &source.subjects).await? {
            return Err(Error::Conflict(
                "This correction is already shared with every account".into(),
            ));
        }
        let id = Uuid::new_v4().to_string();
        let seq: i64 = sqlx::query_scalar(
            "INSERT INTO shared_artist_corrections(id,promoted_by,source_kind,source_change_id,source_association_id,change_floor)
             VALUES(?,?,?,?,?,(SELECT COALESCE(max(seq),0) FROM recording_artist_association_change_order)) RETURNING seq",
        )
        .bind(&id)
        .bind(actor.id())
        .bind(source.kind)
        .bind(&source.change)
        .bind(&source.association)
        .fetch_one(&mut *tx)
        .await?;
        for subject in &source.subjects {
            let raw: Option<String> =
                sqlx::query_scalar("SELECT artist FROM recordings WHERE id=?")
                    .bind(&subject.recording)
                    .fetch_optional(&mut *tx)
                    .await?;
            let raw = raw.ok_or(Error::NotFound)?;
            sqlx::query(
                "INSERT INTO shared_artist_correction_subjects(correction_seq,recording_id,observation_id,provider_artist_id,raw_artist)
                 VALUES(?,?,?,?,?)",
            )
            .bind(seq)
            .bind(&subject.recording)
            .bind(&subject.observation)
            .bind(&subject.artist)
            .bind(raw)
            .execute(&mut *tx)
            .await?;
        }
        sqlx::query("INSERT INTO shared_artist_correction_states(correction_seq,revision,applied,changed_by) VALUES(?,1,1,?)")
            .bind(seq)
            .bind(actor.id())
            .execute(&mut *tx)
            .await?;
        let receipt = SharedArtistCorrectionReceipt {
            request_id: request.request_id,
            correction_id: id,
            revision: 1,
            applied: true,
            recordings: source.subjects.len() as i64,
        };
        save(&mut tx, actor, &digest, &receipt).await?;
        actor.check_session_on(&mut tx).await?;
        tx.commit().await?;
        Ok(receipt)
    }

    async fn accept_correction_undo(
        &self,
        actor: &Actor,
        correction: &str,
        request: UndoSharedArtistCorrection,
    ) -> Result<SharedArtistCorrectionReceipt> {
        actor.require_admin()?;
        if request.request_id.is_nil() || request.expected_revision < 1 {
            return Err(Error::Invalid(
                "Use a nonempty request and a positive revision".into(),
            ));
        }
        let digest = fingerprint("hito-artist-correction-undo:v1", correction, &request)?;
        let mut tx = self.store.pool.begin_with("BEGIN IMMEDIATE").await?;
        actor.check_session_on(&mut tx).await?;
        if let Some(receipt) = replay(&mut tx, actor, request.request_id, &digest).await? {
            return Ok(receipt);
        }
        let row: Option<(i64, i64, bool, i64)> = sqlx::query_as(
            "SELECT correction.seq,state.revision,state.applied,
                (SELECT count(*) FROM shared_artist_correction_subjects s WHERE s.correction_seq=correction.seq)
             FROM shared_artist_corrections correction
             JOIN shared_artist_correction_states state ON state.correction_seq=correction.seq
                AND state.revision=(SELECT max(revision) FROM shared_artist_correction_states WHERE correction_seq=correction.seq)
             WHERE correction.id=?",
        )
        .bind(correction)
        .fetch_optional(&mut *tx)
        .await?;
        let (seq, current, applied, recordings) = row.ok_or(Error::NotFound)?;
        if current != request.expected_revision {
            return Err(stale());
        }
        if !applied {
            let blocked: bool = sqlx::query_scalar(
                "SELECT NOT EXISTS(SELECT 1 FROM shared_artist_correction_subjects mine
                    WHERE mine.correction_seq=? AND mine.retired_reason IS NULL
                        AND NOT EXISTS (SELECT 1 FROM shared_artist_correction_subjects other
                            WHERE other.correction_seq<>mine.correction_seq AND other.retired_reason IS NULL
                                AND other.recording_id=mine.recording_id AND other.observation_id=mine.observation_id
                                AND other.provider_artist_id=mine.provider_artist_id
                                AND (SELECT state.applied FROM shared_artist_correction_states state
                                     WHERE state.correction_seq=other.correction_seq ORDER BY state.revision DESC LIMIT 1)=1))",
            )
            .bind(seq)
            .fetch_one(&mut *tx)
            .await?;
            if blocked {
                return Err(Error::Conflict(
                    "Every recording of this correction is retired or already shared".into(),
                ));
            }
        }
        sqlx::query("INSERT INTO shared_artist_correction_states(correction_seq,revision,applied,changed_by) VALUES(?,?,?,?)")
            .bind(seq)
            .bind(current + 1)
            .bind(!applied)
            .bind(actor.id())
            .execute(&mut *tx)
            .await?;
        let receipt = SharedArtistCorrectionReceipt {
            request_id: request.request_id,
            correction_id: correction.into(),
            revision: current + 1,
            applied: !applied,
            recordings,
        };
        save(&mut tx, actor, &digest, &receipt).await?;
        actor.check_session_on(&mut tx).await?;
        tx.commit().await?;
        Ok(receipt)
    }
}

fn finished(message: &str, result: &Result<SharedArtistCorrectionReceipt>, started: Instant) {
    tracing::info!(
        outcome = result.as_ref().map_or_else(|e| e.code(), |_| "accepted"),
        recordings = result.as_ref().map_or(0, |receipt| receipt.recordings),
        elapsed_ms = started.elapsed().as_millis() as u64,
        "{message}"
    );
}
