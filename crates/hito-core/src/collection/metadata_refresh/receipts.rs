use super::{
    ImportedPlaylistMetadataRefresh, RefreshImportedPlaylistMetadata, authority::identity,
};
use crate::{Error, Result};
use sha2::{Digest, Sha256};
use sqlx::SqliteConnection;
use uuid::Uuid;

const MAX_ACCOUNT_RECEIPTS: i64 = 4096;
const MAX_RECEIPTS: i64 = 65_536;

pub(super) fn fingerprint(
    playlist: &str,
    request: &RefreshImportedPlaylistMetadata,
) -> Result<String> {
    let bytes = serde_json::to_vec(&("hito-playlist-metadata-refresh:v1", playlist, request))
        .map_err(|_| Error::Internal("Playlist metadata request could not be encoded".into()))?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}

#[derive(sqlx::FromRow)]
struct Receipt {
    request_digest: String,
    playlist_id: String,
    updated_recordings: i64,
    unchanged_recordings: i64,
    unavailable_recordings: i64,
}

pub(super) async fn lookup(
    connection: &mut SqliteConnection,
    account: &str,
    request_id: Uuid,
    digest: &str,
) -> Result<Option<ImportedPlaylistMetadataRefresh>> {
    let previous: Option<Receipt> = sqlx::query_as(
        "SELECT request_digest,playlist_id,updated_recordings,unchanged_recordings,unavailable_recordings
         FROM playlist_metadata_refresh_receipts WHERE user_id = ? AND request_id = ?",
    ).bind(account).bind(request_id.to_string()).fetch_optional(connection).await?;
    let Some(previous) = previous else {
        return Ok(None);
    };
    if previous.request_digest != digest {
        return Err(Error::Conflict(
            "A playlist metadata request ID was reused with different contents".into(),
        ));
    }
    Ok(Some(ImportedPlaylistMetadataRefresh {
        request_id,
        playlist_id: identity(&previous.playlist_id)?,
        updated_recordings: count(previous.updated_recordings)?,
        unchanged_recordings: count(previous.unchanged_recordings)?,
        unavailable_recordings: count(previous.unavailable_recordings)?,
    }))
}

pub(super) async fn capacity(connection: &mut SqliteConnection, account: &str) -> Result<()> {
    let (account_count, total): (i64, i64) = sqlx::query_as(
        "SELECT (SELECT count(*) FROM playlist_metadata_refresh_receipts WHERE user_id = ?),
            (SELECT count(*) FROM playlist_metadata_refresh_receipts)",
    )
    .bind(account)
    .fetch_one(connection)
    .await?;
    if account_count >= MAX_ACCOUNT_RECEIPTS || total >= MAX_RECEIPTS {
        return Err(Error::Conflict(
            "Playlist metadata request history is full. Existing receipts have been retained."
                .into(),
        ));
    }
    Ok(())
}

pub(super) async fn save(
    connection: &mut SqliteConnection,
    account: &str,
    digest: &str,
    receipt: &ImportedPlaylistMetadataRefresh,
    sequence: Option<i64>,
) -> Result<()> {
    sqlx::query("INSERT INTO playlist_metadata_refresh_receipts(user_id,request_id,request_digest,playlist_id,
        updated_recordings,unchanged_recordings,unavailable_recordings,publication_sequence) VALUES (?,?,?,?,?,?,?,?)")
        .bind(account).bind(receipt.request_id.to_string()).bind(digest).bind(receipt.playlist_id.to_string())
        .bind(i64::from(receipt.updated_recordings)).bind(i64::from(receipt.unchanged_recordings)).bind(i64::from(receipt.unavailable_recordings))
        .bind(sequence).execute(connection).await?;
    Ok(())
}

fn count(value: i64) -> Result<u32> {
    u32::try_from(value)
        .map_err(|_| Error::Internal("Stored metadata receipt has an invalid count".into()))
}
