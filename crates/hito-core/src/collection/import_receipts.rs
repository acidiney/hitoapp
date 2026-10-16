use super::import::ImportProviderPlaylist;
use crate::{Error, Result};
use serde::Serialize;
use sha2::{Digest, Sha256};
use sqlx::SqliteConnection;
use utoipa::ToSchema;
use uuid::Uuid;

#[derive(Serialize, ToSchema)]
pub struct PlaylistImportReceipt {
    pub request_id: Uuid,
    pub playlist_id: Uuid,
}

pub(super) fn fingerprint(installation: &str, request: &ImportProviderPlaylist) -> Result<String> {
    let contents = serde_json::to_vec(&("hito-playlist-import:v1", installation, request))
        .map_err(|_| Error::Internal("Playlist import request could not be encoded".into()))?;
    Ok(format!("{:x}", Sha256::digest(contents)))
}

pub(super) async fn lookup(
    connection: &mut SqliteConnection,
    account: &str,
    request_id: Uuid,
    digest: &str,
) -> Result<Option<PlaylistImportReceipt>> {
    let row: Option<(String, String)> = sqlx::query_as(
        "SELECT request_digest, playlist_id FROM playlist_import_receipts WHERE user_id = ? AND request_id = ?",
    ).bind(account).bind(request_id.to_string()).fetch_optional(connection).await?;
    let Some((previous, playlist)) = row else {
        return Ok(None);
    };
    if previous != digest {
        return Err(Error::Conflict(
            "A playlist import request ID was reused with different contents".into(),
        ));
    }
    let playlist_id = Uuid::parse_str(&playlist)
        .map_err(|_| Error::Internal("Stored playlist import receipt is invalid".into()))?;
    Ok(Some(PlaylistImportReceipt {
        request_id,
        playlist_id,
    }))
}

pub(super) async fn save(
    connection: &mut SqliteConnection,
    account: &str,
    digest: &str,
    receipt: &PlaylistImportReceipt,
    publication_sequence: i64,
) -> Result<()> {
    sqlx::query("INSERT INTO playlist_import_receipts(user_id, request_id, request_digest, playlist_id, publication_sequence) VALUES (?, ?, ?, ?, ?)")
        .bind(account).bind(receipt.request_id.to_string()).bind(digest)
        .bind(receipt.playlist_id.to_string()).bind(publication_sequence).execute(connection).await?;
    Ok(())
}
