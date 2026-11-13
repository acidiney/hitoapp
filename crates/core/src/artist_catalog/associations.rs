use super::{
    ArtistAssociation, ArtistAssociationEvidence, ArtistCreditRole, AssociateRecordingArtist,
    CoverageEvidence, CreditEvidence, CreditEvidenceKind, MAX_ASSOCIATIONS,
    MAX_RETAINED_ASSOCIATIONS, MetadataProvider, REQUEST_BUDGET, association_changes,
    authority::{self, Credit},
    credit_target::CreditTarget,
    policy, provider,
};
use crate::{Actor, ArtistCatalog, Error, Result, identity::recording_access};
use sqlx::SqliteConnection;
use std::time::Instant;
use uuid::Uuid;

pub(crate) const LINKED: &str = "COALESCE((SELECT CASE WHEN change.linked=1 AND assoc.credit_target='whole_credit' AND assoc.provider_kind='builtin_musicbrainz'
        AND EXISTS (SELECT 1 FROM shared_artist_correction_subjects subject
            JOIN shared_artist_corrections correction ON correction.seq=subject.correction_seq
            WHERE subject.retired_reason IS NULL AND subject.recording_id=assoc.recording_id AND subject.observation_id=assoc.observation_id
                AND subject.provider_artist_id=assoc.provider_artist_id AND (SELECT ordered.seq FROM recording_artist_association_change_order ordered
                    WHERE ordered.user_id=change.user_id AND ordered.recording_id=change.recording_id
                        AND ordered.revision=change.revision)<=correction.change_floor
                AND (SELECT state.applied FROM shared_artist_correction_states state
                    WHERE state.correction_seq=correction.seq ORDER BY state.revision DESC LIMIT 1)=1)
        THEN 0 ELSE change.linked END
    FROM recording_artist_association_changes change
    WHERE change.user_id=assoc.user_id AND change.association_id=assoc.id ORDER BY change.revision DESC LIMIT 1),1)=1";

const MATCHED: &str = "COALESCE((SELECT matched.evidence_kind FROM artist_association_matches matched WHERE matched.association_id=assoc.id),
    (SELECT matched.evidence_kind FROM artist_association_provider_matches matched WHERE matched.association_id=assoc.id),
    (SELECT 'recording_id' FROM artist_association_recording_id_matches matched WHERE matched.association_id=assoc.id),
    (SELECT matched.evidence_kind FROM artist_association_name_matches matched WHERE matched.association_id=assoc.id),
    (SELECT 'credit_'||matched.evidence_kind FROM artist_association_credit_matches matched WHERE matched.association_id=assoc.id),
    (SELECT 'coverage' FROM artist_association_coverage_matches matched WHERE matched.association_id=assoc.id),
    (SELECT matched.evidence_kind FROM artist_association_candidate_confirmations matched WHERE matched.association_id=assoc.id)) AS matched,
    (SELECT matched.source_tracks FROM artist_association_credit_matches matched WHERE matched.association_id=assoc.id) AS credit_tracks,
    (SELECT matched.matched_tracks FROM artist_association_coverage_matches matched WHERE matched.association_id=assoc.id) AS coverage_matched,
    (SELECT matched.probed_tracks FROM artist_association_coverage_matches matched WHERE matched.association_id=assoc.id) AS coverage_probed";

#[derive(sqlx::FromRow)]
struct Stored {
    id: String,
    user_id: String,
    library_id: String,
    recording_id: String,
    artist_id: String,
    source_id: Option<String>,
    observation_id: String,
    provider_kind: String,
    plugin_installation_id: Option<String>,
    provider_artist_id: String,
    revision: i64,
    matched: Option<String>,
    credit_tracks: Option<i64>,
    coverage_matched: Option<i64>,
    coverage_probed: Option<i64>,
}

fn credit_evidence(matched: Option<&str>, tracks: Option<i64>) -> Option<CreditEvidence> {
    let kind = match matched?.strip_prefix("credit_")? {
        "file_tag_artist_id" => CreditEvidenceKind::FileTagArtistId,
        "isrc" => CreditEvidenceKind::Isrc,
        "provider_url" => CreditEvidenceKind::ProviderUrl,
        "name_discography" => CreditEvidenceKind::NameDiscography,
        _ => return None,
    };
    Some(CreditEvidence {
        kind,
        tracks: tracks?,
    })
}

impl Stored {
    fn project(self) -> Result<ArtistAssociation> {
        let provider = match (self.provider_kind.as_str(), self.plugin_installation_id) {
            ("builtin_musicbrainz", None) => MetadataProvider::BuiltinMusicBrainz,
            ("http_plugin", Some(installation_id)) => {
                MetadataProvider::HttpPlugin { installation_id }
            }
            _ => {
                return Err(Error::Internal(
                    "Stored artist association is invalid".into(),
                ));
            }
        };
        Ok(ArtistAssociation {
            id: self.id,
            account_id: self.user_id,
            library_id: self.library_id,
            recording_id: self.recording_id,
            artist_id: self.artist_id,
            source_id: self.source_id,
            observation_id: self.observation_id,
            role: ArtistCreditRole::RecordingArtist,
            provider,
            provider_artist_id: self.provider_artist_id,
            evidence: match self.matched.as_deref() {
                None => ArtistAssociationEvidence::UserConfirmed,
                Some("file_tag_artist_id") => ArtistAssociationEvidence::MatchedFileTagArtistId,
                Some("recording_id") => ArtistAssociationEvidence::MatchedRecordingId,
                Some("isrc") => ArtistAssociationEvidence::MatchedIsrc,
                Some("provider_url") => ArtistAssociationEvidence::MatchedProviderUrl,
                Some("name_discography") => ArtistAssociationEvidence::MatchedNameDiscography,
                Some("coverage") => ArtistAssociationEvidence::MatchedCreditCoverage,
                Some("user_confirmed_candidate") => {
                    ArtistAssociationEvidence::UserConfirmedCandidate
                }
                Some(kind) if kind.starts_with("credit_") => {
                    ArtistAssociationEvidence::MatchedCreditEvidence
                }
                Some(_) => {
                    return Err(Error::Internal(
                        "Stored artist association provenance is invalid".into(),
                    ));
                }
            },
            credit_evidence: credit_evidence(self.matched.as_deref(), self.credit_tracks),
            coverage_evidence: self.coverage_matched.zip(self.coverage_probed).map(
                |(matched_tracks, probed_tracks)| CoverageEvidence {
                    matched_tracks,
                    probed_tracks,
                },
            ),
            revision: self.revision,
        })
    }
}

impl ArtistCatalog {
    #[tracing::instrument(level = "debug", skip_all, fields(operation = "artist_associations", request_id = %Uuid::new_v4()))]
    pub async fn recording_artist_associations(
        &self,
        actor: &Actor,
        recording: &str,
    ) -> Result<Vec<ArtistAssociation>> {
        let started = Instant::now();
        let result = tokio::time::timeout(REQUEST_BUDGET, async {
            let mut tx = self.store.pool.begin().await?;
            actor.check_session_on(&mut tx).await?;
            recording_access(&mut tx, actor, recording).await?;
            let rows: Vec<Stored> = sqlx::query_as(&format!(
                "SELECT assoc.*, {MATCHED} FROM recording_artist_associations assoc
                WHERE assoc.user_id=? AND assoc.recording_id=? AND {LINKED} ORDER BY assoc.created_at,assoc.id LIMIT ?"
            ))
            .bind(actor.id())
            .bind(recording)
            .bind(MAX_ASSOCIATIONS)
            .fetch_all(&mut *tx)
            .await?;
            let associations = rows
                .into_iter()
                .map(Stored::project)
                .collect::<Result<Vec<_>>>()?;
            tx.commit().await?;
            Ok(associations)
        })
        .await
        .unwrap_or(Err(Error::Upstream(
            "Artist associations exceeded the time limit",
        )));
        tracing::debug!(
            outcome = result
                .as_ref()
                .map(|_| "success")
                .unwrap_or_else(|error| error.code()),
            association_count = result.as_ref().map(|items| items.len()).unwrap_or(0),
            elapsed_ms = started.elapsed().as_millis() as u64,
            "Artist association read completed"
        );
        result
    }

    #[tracing::instrument(level = "info", skip_all, fields(operation = "artist_association_review", request_id = %Uuid::new_v4()))]
    pub async fn associate_recording_artist(
        &self,
        actor: &Actor,
        recording: &str,
        request: AssociateRecordingArtist,
    ) -> Result<ArtistAssociation> {
        let started = Instant::now();
        let result = tokio::time::timeout(
            REQUEST_BUDGET,
            self.review_artist(actor, recording, request),
        )
        .await
        .unwrap_or(Err(Error::Upstream(
            "Artist association review exceeded the time limit",
        )));
        tracing::info!(
            outcome = result
                .as_ref()
                .map(|_| "success")
                .unwrap_or_else(|error| error.code()),
            elapsed_ms = started.elapsed().as_millis() as u64,
            "Artist association review completed"
        );
        result
    }

    async fn review_artist(
        &self,
        actor: &Actor,
        recording: &str,
        request: AssociateRecordingArtist,
    ) -> Result<ArtistAssociation> {
        actor.check_session(&self.store).await?;
        provider::validate_artist_id(&request.provider, &request.provider_artist_id)?;
        let mut tx = self.store.pool.begin_with("BEGIN IMMEDIATE").await?;
        let credit =
            Credit::capture_for_artist(&mut tx, actor, recording, &request.artist_id).await?;
        tracing::debug!(
            credit_target = credit.target.storage_kind(),
            "Artist association target captured"
        );
        if credit.artist_id != request.artist_id
            || credit.source_id != request.source_id
            || credit.observation_id != request.observation_id
        {
            return Err(authority::changed());
        }
        let policy = policy::read(&mut tx, actor).await?;
        if policy.revision != request.policy_revision
            || !policy::selected(&policy, &request.provider)
        {
            return Err(policy::changed());
        }
        policy::approved(&mut tx, actor, &request.provider).await?;
        let association = retain(&mut tx, actor, recording, &credit.library_id, &request).await?;
        tx.commit().await?;
        Ok(association)
    }
}

pub(super) async fn existing(
    connection: &mut SqliteConnection,
    actor: &Actor,
    recording: &str,
    request: &AssociateRecordingArtist,
) -> Result<Option<String>> {
    let credit =
        Credit::capture_for_artist(connection, actor, recording, &request.artist_id).await?;
    existing_for_target(connection, actor, recording, request, &credit.target).await
}

async fn existing_for_target(
    connection: &mut SqliteConnection,
    actor: &Actor,
    recording: &str,
    request: &AssociateRecordingArtist,
    target: &CreditTarget,
) -> Result<Option<String>> {
    let (kind, installation) = authority::provider_columns(&request.provider);
    Ok(sqlx::query_scalar(
        "SELECT id FROM recording_artist_associations
            WHERE user_id=? AND recording_id=? AND artist_id=? AND source_id IS ? AND observation_id=?
                AND provider_kind=? AND plugin_installation_id IS ? AND provider_artist_id=? AND credit_target=?",
    )
    .bind(actor.id())
    .bind(recording)
    .bind(&request.artist_id)
    .bind(&request.source_id)
    .bind(&request.observation_id)
    .bind(kind)
    .bind(installation)
    .bind(&request.provider_artist_id)
    .bind(target.storage_kind())
    .fetch_optional(connection)
    .await?)
}

pub(super) async fn unlinked_match(
    connection: &mut SqliteConnection,
    actor: &Actor,
    recording: &str,
    request: &AssociateRecordingArtist,
) -> Result<bool> {
    match existing(connection, actor, recording, request).await? {
        Some(id) => Ok(!association_changes::is_linked(connection, &id).await?),
        None => Ok(false),
    }
}

pub(super) async fn retain(
    connection: &mut SqliteConnection,
    actor: &Actor,
    recording: &str,
    library: &str,
    request: &AssociateRecordingArtist,
) -> Result<ArtistAssociation> {
    let credit =
        Credit::capture_for_artist(connection, actor, recording, &request.artist_id).await?;
    retain_target(
        connection,
        actor,
        recording,
        library,
        request,
        &credit.target,
    )
    .await
}

pub(super) async fn retain_target(
    connection: &mut SqliteConnection,
    actor: &Actor,
    recording: &str,
    library: &str,
    request: &AssociateRecordingArtist,
    target: &CreditTarget,
) -> Result<ArtistAssociation> {
    let credit = Credit::capture_target(connection, actor, recording, target).await?;
    if credit.library_id != library
        || credit.artist_id != request.artist_id
        || credit.source_id != request.source_id
        || credit.observation_id != request.observation_id
    {
        return Err(authority::changed());
    }
    let (kind, installation) = authority::provider_columns(&request.provider);
    let id = if let Some(existing) =
        existing_for_target(connection, actor, recording, request, &credit.target).await?
    {
        if !association_changes::is_linked(connection, &existing).await? {
            association_changes::ensure_room(connection, actor, recording).await?;
            association_changes::append(connection, actor, recording, &existing, true).await?;
        }
        existing
    } else {
        association_changes::ensure_room(connection, actor, recording).await?;
        let retained: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM recording_artist_associations WHERE user_id=? AND recording_id=?",
        )
        .bind(actor.id())
        .bind(recording)
        .fetch_one(&mut *connection)
        .await?;
        if retained >= MAX_RETAINED_ASSOCIATIONS {
            return Err(Error::Conflict(
                "This recording has retained too many artist associations".into(),
            ));
        }
        let id = Uuid::new_v4().to_string();
        sqlx::query("INSERT INTO recording_artist_associations(id,user_id,library_id,recording_id,artist_id,
                source_id,observation_id,role,provider_kind,plugin_installation_id,provider_artist_id,evidence,credit_target)
                VALUES (?,?,?,?,?,?,?,'recording_artist',?,?,?,'user_confirmed',?)")
            .bind(&id).bind(actor.id()).bind(library).bind(recording).bind(&request.artist_id)
            .bind(&request.source_id).bind(&request.observation_id).bind(kind).bind(installation)
            .bind(&request.provider_artist_id).bind(credit.target.storage_kind()).execute(&mut *connection).await?;
        association_changes::append(connection, actor, recording, &id, true).await?;
        id
    };
    load(connection, actor, recording, &id).await
}

pub(super) async fn load(
    connection: &mut SqliteConnection,
    actor: &Actor,
    recording: &str,
    association: &str,
) -> Result<ArtistAssociation> {
    let row: Stored = sqlx::query_as(&format!(
        "SELECT assoc.*, {MATCHED} FROM recording_artist_associations assoc
        WHERE assoc.id=? AND assoc.user_id=? AND assoc.recording_id=? AND {LINKED}"
    ))
    .bind(association)
    .bind(actor.id())
    .bind(recording)
    .fetch_optional(connection)
    .await?
    .ok_or(Error::NotFound)?;
    row.project()
}
