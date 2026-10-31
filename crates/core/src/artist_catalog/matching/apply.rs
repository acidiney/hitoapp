//! Decisions shared by every automatic match: personal decisions are never overridden, and a
//! match is recorded as a reviewed association plus a chosen artist with its evidence kind.
use crate::artist_catalog::{
    ArtistCreditRole, AssociateRecordingArtist, MetadataProvider, association_changes,
    associations,
    authority::{Credit, provider_columns},
    identity::choices,
};
use crate::{Actor, Error, Result};
use sqlx::{Connection, SqliteConnection};

/// A choice written by an earlier automatic match that no decision of the account has replaced,
/// and whose observation is no longer current. Only such a choice may be matched again.
pub(super) const REPLACEABLE: &str = "(EXISTS (SELECT 1 FROM (
            SELECT association_id,receipt_order FROM artist_association_matches
            UNION ALL SELECT association_id,receipt_order FROM artist_association_provider_matches
            UNION ALL SELECT association_id,receipt_order FROM artist_association_name_matches
            UNION ALL SELECT association_id,receipt_order FROM artist_association_credit_matches
            UNION ALL SELECT association_id,receipt_order FROM artist_association_coverage_matches) mt
        WHERE mt.association_id=c.association_id AND mt.receipt_order=c.selection_order)
    AND (c.retired=1 OR c.observation_id IS NOT r.metadata_observation_id))";

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum EvidenceKind {
    FileTagArtistId,
    Isrc,
    ProviderUrl,
    NameDiscography,
    /// The credit's other tracks, matched because this many of its tracks were individually
    /// matched from the originating evidence.
    Credit(Origin, u32),
    /// The credit's remaining tracks, matched because the artist listed this many of the credit's
    /// tracks of this many that were probed.
    Coverage(u32, u32),
    /// A candidate the account confirmed in the review queue.
    UserConfirmedCandidate,
}

/// The evidence a credit-level match started from.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Origin {
    FileTagArtistId,
    Isrc,
    ProviderUrl,
    NameDiscography,
}

impl Origin {
    fn name(self) -> &'static str {
        match self {
            Self::FileTagArtistId => "file_tag_artist_id",
            Self::Isrc => "isrc",
            Self::ProviderUrl => "provider_url",
            Self::NameDiscography => "name_discography",
        }
    }
}

impl EvidenceKind {
    fn table(self) -> &'static str {
        match self {
            Self::FileTagArtistId => "artist_association_matches",
            Self::Isrc | Self::ProviderUrl => "artist_association_provider_matches",
            Self::NameDiscography => "artist_association_name_matches",
            Self::Credit(..) => "artist_association_credit_matches",
            Self::Coverage(..) => "artist_association_coverage_matches",
            Self::UserConfirmedCandidate => "artist_association_candidate_confirmations",
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::FileTagArtistId => Origin::FileTagArtistId.name(),
            Self::Isrc => Origin::Isrc.name(),
            Self::ProviderUrl => Origin::ProviderUrl.name(),
            Self::NameDiscography => Origin::NameDiscography.name(),
            Self::Credit(origin, _) => origin.name(),
            Self::Coverage(..) => "name_coverage",
            Self::UserConfirmedCandidate => "user_confirmed_candidate",
        }
    }

    /// The evidence an individual match was made from, as the origin of its credit's other tracks.
    pub(super) fn origin(self) -> Option<Origin> {
        match self {
            Self::FileTagArtistId => Some(Origin::FileTagArtistId),
            Self::Isrc => Some(Origin::Isrc),
            Self::ProviderUrl => Some(Origin::ProviderUrl),
            Self::NameDiscography
            | Self::Credit(..)
            | Self::Coverage(..)
            | Self::UserConfirmedCandidate => None,
        }
    }
}

pub(super) struct Pending {
    pub(super) recording: String,
    pub(super) artist: String,
    association: String,
    created: bool,
    revision: i64,
    credit: Credit,
    pub(super) kind: EvidenceKind,
}

impl Pending {
    pub(super) fn credit_artist(&self) -> &str {
        &self.credit.artist_id
    }

    pub(super) fn credit_name(&self) -> &str {
        &self.credit.credited_name
    }
}

pub(super) enum Planned {
    Match(Box<Pending>),
    Declined { remember: bool },
}

pub(super) async fn plan(
    tx: &mut SqliteConnection,
    actor: &Actor,
    recording: &str,
    artist: &str,
    policy_revision: i64,
    kind: EvidenceKind,
) -> Result<Planned> {
    let choice: Option<(i64, bool)> = sqlx::query_as(&format!(
        "SELECT c.revision,{REPLACEABLE} FROM recording_artist_identity_choices c
         JOIN recordings r ON r.id=c.recording_id WHERE c.user_id=? AND c.recording_id=?"
    ))
    .bind(actor.id())
    .bind(recording)
    .fetch_optional(&mut *tx)
    .await?;
    let revision = match choice {
        Some((_, false)) => return Ok(Planned::Declined { remember: false }),
        Some((revision, true)) => revision,
        None => 0,
    };
    let Some(revision) = revision.checked_add(1) else {
        return Ok(Planned::Declined { remember: false });
    };
    let credit = match Credit::capture(tx, actor, recording).await {
        Ok(credit) if credit.credit_operation.is_none() => credit,
        Ok(_) => return Ok(Planned::Declined { remember: true }),
        Err(Error::NotFound) => return Ok(Planned::Declined { remember: false }),
        Err(error) => return Err(error),
    };
    let hidden: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM shared_artist_correction_subjects subject
            WHERE subject.retired_reason IS NULL AND subject.recording_id=? AND subject.observation_id=?
                AND subject.provider_artist_id=?
                AND (SELECT state.applied FROM shared_artist_correction_states state
                    WHERE state.correction_seq=subject.correction_seq ORDER BY state.revision DESC LIMIT 1)=1)",
    )
    .bind(recording)
    .bind(&credit.observation_id)
    .bind(artist)
    .fetch_one(&mut *tx)
    .await?;
    if hidden {
        return Ok(Planned::Declined { remember: true });
    }
    let request = AssociateRecordingArtist {
        artist_id: credit.artist_id.clone(),
        source_id: credit.source_id.clone(),
        observation_id: credit.observation_id.clone(),
        role: ArtistCreditRole::RecordingArtist,
        provider: MetadataProvider::BuiltinMusicBrainz,
        provider_artist_id: artist.into(),
        policy_revision,
    };
    let existing = associations::existing(tx, actor, recording, &request).await?;
    if let Some(id) = &existing
        && !association_changes::is_linked(tx, id).await?
    {
        return Ok(Planned::Declined { remember: true });
    }
    let mut attempt = tx.begin().await?;
    match associations::retain(&mut attempt, actor, recording, &credit.library_id, &request).await {
        Ok(association) => {
            attempt.commit().await?;
            Ok(Planned::Match(Box::new(Pending {
                recording: recording.into(),
                artist: artist.into(),
                association: association.id,
                created: existing.is_none(),
                revision,
                credit,
                kind,
            })))
        }
        Err(Error::Conflict(_) | Error::NotFound) => {
            attempt.rollback().await?;
            Ok(Planned::Declined { remember: true })
        }
        Err(error) => Err(error),
    }
}

pub(super) async fn commit(
    tx: &mut SqliteConnection,
    actor: &Actor,
    pending: &[Pending],
    order: i64,
) -> Result<()> {
    let (provider, installation) = provider_columns(&MetadataProvider::BuiltinMusicBrainz);
    for item in pending {
        let group =
            choices::ensure_group(&mut *tx, actor, provider, installation, &item.artist).await?;
        choices::store(
            &mut *tx,
            actor,
            &item.recording,
            item.revision,
            &item.association,
            &group,
            &item.credit,
            order,
        )
        .await?;
        if item.created {
            let (columns, marks) = match item.kind {
                EvidenceKind::Credit(..) => (
                    "association_id,evidence_kind,receipt_order,source_tracks",
                    "?,?,?,?",
                ),
                EvidenceKind::Coverage(..) => (
                    "association_id,receipt_order,matched_tracks,probed_tracks",
                    "?,?,?,?",
                ),
                EvidenceKind::NameDiscography => {
                    ("association_id,evidence_kind,receipt_order,rule", "?,?,?,?")
                }
                _ => ("association_id,evidence_kind,receipt_order", "?,?,?"),
            };
            let sql = format!(
                "INSERT INTO {}({columns}) VALUES({marks})",
                item.kind.table()
            );
            let insert = sqlx::query(&sql).bind(&item.association);
            let insert = match item.kind {
                EvidenceKind::Coverage(matched, probed) => insert
                    .bind(order)
                    .bind(i64::from(matched))
                    .bind(i64::from(probed)),
                EvidenceKind::Credit(_, tracks) => insert
                    .bind(item.kind.name())
                    .bind(order)
                    .bind(i64::from(tracks)),
                EvidenceKind::NameDiscography => insert
                    .bind(item.kind.name())
                    .bind(order)
                    .bind(super::name_discography::NAME_RULE),
                _ => insert.bind(item.kind.name()).bind(order),
            };
            insert.execute(&mut *tx).await?;
        }
    }
    Ok(())
}
