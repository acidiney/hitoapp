use super::{
    assets,
    availability::automatic_artwork_allowed,
    cover_art::{CoverArtKind, CoverArtLookup},
    credited_artist,
};
use crate::{Error, Result};
use sqlx::SqliteConnection;

pub(super) const RETRY_AFTER_SECONDS: i64 = 86_400;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum LookupState {
    Queued,
    Running,
    Ready,
    Missing,
    Unavailable,
}

impl LookupState {
    pub(super) fn parse(value: &str) -> Result<Self> {
        match value {
            "queued" => Ok(Self::Queued),
            "running" => Ok(Self::Running),
            "ready" => Ok(Self::Ready),
            "missing" => Ok(Self::Missing),
            "unavailable" => Ok(Self::Unavailable),
            _ => Err(Error::Internal("Stored cover art state is invalid".into())),
        }
    }

    pub(super) fn stored(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Running => "running",
            Self::Ready => "ready",
            Self::Missing => "missing",
            Self::Unavailable => "unavailable",
        }
    }

    pub(super) fn is_pending(self) -> bool {
        matches!(self, Self::Queued | Self::Running)
    }

    pub(super) fn is_answered_absent(self) -> bool {
        matches!(self, Self::Missing | Self::Unavailable)
    }
}

pub(super) struct StoredLookup {
    pub(super) state: LookupState,
    pub(super) hash: Option<String>,
    pub(super) aged: bool,
}

pub(super) async fn read(
    connection: &mut SqliteConnection,
    account: &str,
    kind: CoverArtKind,
    mbid: &str,
) -> Result<Option<StoredLookup>> {
    let row: Option<(String, Option<String>, bool)> = sqlx::query_as(
        "SELECT state,content_hash,updated_at < unixepoch() - ? FROM cover_art_lookups
         WHERE user_id = ? AND kind = ? AND mbid = ?",
    )
    .bind(RETRY_AFTER_SECONDS)
    .bind(account)
    .bind(kind.stored())
    .bind(mbid)
    .fetch_optional(connection)
    .await?;
    row.map(|(state, hash, aged)| {
        Ok(StoredLookup {
            state: LookupState::parse(&state)?,
            hash,
            aged,
        })
    })
    .transpose()
}

pub(super) async fn lookup_status(
    connection: &mut SqliteConnection,
    account: &str,
    kind: CoverArtKind,
    mbid: &str,
) -> Result<CoverArtLookup> {
    let Some(lookup) = read(connection, account, kind, mbid).await? else {
        return Ok(CoverArtLookup::Unknown);
    };
    Ok(match (lookup.state, lookup.hash) {
        (LookupState::Ready, Some(hash)) => {
            if still_servable(connection, account, kind, mbid).await? {
                CoverArtLookup::Ready(assets::ready_asset(connection, account, &hash).await?)
            } else {
                CoverArtLookup::Unknown
            }
        }
        (state, _) if state.is_answered_absent() && lookup.aged && kind.is_artist_photo() => {
            CoverArtLookup::Unknown
        }
        (LookupState::Missing, _) => CoverArtLookup::Missing,
        (LookupState::Unavailable, _) => CoverArtLookup::Unavailable,
        _ => CoverArtLookup::Pending,
    })
}

async fn still_servable(
    connection: &mut SqliteConnection,
    account: &str,
    kind: CoverArtKind,
    mbid: &str,
) -> Result<bool> {
    if kind == CoverArtKind::CreditedArtist
        && credited_artist::target(connection, account, mbid)
            .await?
            .is_none()
    {
        return Ok(false);
    }
    if kind.is_artist_photo() && !automatic_artwork_allowed(connection, account).await? {
        return Ok(false);
    }
    Ok(true)
}
