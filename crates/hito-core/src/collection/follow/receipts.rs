use super::{FollowProviderPlaylist, PlaylistFollowReceipt, PlaylistFollowRefresh};
use crate::{Error, Result};
use sha2::{Digest, Sha256};
use sqlx::SqliteConnection;
use uuid::Uuid;

const MAX_ACCOUNT_REFRESH_RECEIPTS: i64 = 256;
const MAX_REFRESH_RECEIPTS: i64 = 4096;

pub(super) fn fingerprint(installation: &str, request: &FollowProviderPlaylist) -> Result<String> {
    let bytes = serde_json::to_vec(&("hito-playlist-follow:v1", installation, request))
        .map_err(|_| Error::Internal("Playlist follow request could not be encoded".into()))?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}

pub(super) async fn lookup(
    connection: &mut SqliteConnection,
    account: &str,
    request: Uuid,
    digest: &str,
) -> Result<Option<PlaylistFollowReceipt>> {
    let previous: Option<(String, String)> = sqlx::query_as("SELECT request_digest,playlist_id FROM playlist_follow_receipts WHERE user_id = ? AND request_id = ?")
        .bind(account).bind(request.to_string()).fetch_optional(connection).await?;
    previous
        .map(|(saved, playlist)| {
            if saved != digest {
                return Err(Error::Conflict(
                    "A follow request ID was reused with different contents".into(),
                ));
            }
            Ok(PlaylistFollowReceipt {
                request_id: request,
                playlist_id: identity(&playlist)?,
            })
        })
        .transpose()
}

pub(super) async fn refresh(
    connection: &mut SqliteConnection,
    account: &str,
    playlist: &str,
    request: Uuid,
) -> Result<Option<PlaylistFollowRefresh>> {
    let previous: Option<(String, String)> = sqlx::query_as("SELECT playlist_id,job_id FROM playlist_follow_refresh_receipts WHERE user_id = ? AND request_id = ?")
        .bind(account).bind(request.to_string()).fetch_optional(connection).await?;
    previous
        .map(|(saved, job)| {
            if saved != playlist {
                return Err(Error::Conflict(
                    "A refresh request ID was reused for a different playlist".into(),
                ));
            }
            Ok(PlaylistFollowRefresh {
                request_id: request,
                playlist_id: identity(&saved)?,
                job_id: identity(&job)?,
            })
        })
        .transpose()
}

pub(super) async fn check_refresh_capacity(
    connection: &mut SqliteConnection,
    account: &str,
) -> Result<()> {
    let (account_count, total): (i64, i64) = sqlx::query_as(
        "SELECT coalesce(sum(user_id = ?),0),count(*) FROM playlist_follow_refresh_receipts",
    )
    .bind(account)
    .fetch_one(connection)
    .await?;
    if account_count >= MAX_ACCOUNT_REFRESH_RECEIPTS || total >= MAX_REFRESH_RECEIPTS {
        tracing::debug!(
            account_receipts = account_count,
            total_receipts = total,
            outcome = "capacity_reached",
            "Manual follow refresh request rejected"
        );
        return Err(Error::Conflict(
            "The retained manual playlist refresh request limit has been reached".into(),
        ));
    }
    Ok(())
}

pub(super) fn identity(value: &str) -> Result<Uuid> {
    Uuid::parse_str(value)
        .map_err(|_| Error::Internal("Stored playlist identity is invalid".into()))
}
