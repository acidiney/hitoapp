use crate::{Actor, Error, Result};
use serde::{Serialize, de::DeserializeOwned};
use sha2::{Digest, Sha256};
use sqlx::SqliteConnection;
use uuid::Uuid;

pub(super) fn fingerprint(
    command: &str,
    recording: &str,
    request: &impl Serialize,
) -> Result<String> {
    let encoded = serde_json::to_vec(&(command, recording, request))
        .map_err(|_| Error::Internal("Artist identity command could not be encoded".into()))?;
    Ok(format!("{:x}", Sha256::digest(encoded)))
}

pub(super) async fn replay<T: DeserializeOwned>(
    connection: &mut SqliteConnection,
    actor: &Actor,
    request_id: Uuid,
    digest: &str,
) -> Result<Option<T>> {
    let previous: Option<(String, String)> = sqlx::query_as(
        "SELECT request_digest,receipt FROM artist_identity_choice_receipts WHERE user_id=? AND request_id=?",
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
            "This artist identity request was already used with different contents".into(),
        ));
    }
    let receipt = serde_json::from_str(&receipt)
        .map_err(|_| Error::Internal("Stored artist identity receipt is invalid".into()))?;
    tracing::debug!(
        outcome = "replayed",
        "Artist identity acknowledgement replayed"
    );
    Ok(Some(receipt))
}

pub(super) async fn save(
    connection: &mut SqliteConnection,
    actor: &Actor,
    digest: &str,
    request_id: Uuid,
    receipt: &impl Serialize,
) -> Result<i64> {
    let encoded = serde_json::to_string(receipt)
        .map_err(|_| Error::Internal("Artist identity receipt could not be encoded".into()))?;
    let order = sqlx::query_scalar("INSERT INTO artist_identity_choice_receipts(user_id,request_id,request_digest,receipt) VALUES(?,?,?,?) RETURNING acceptance_order")
        .bind(actor.id()).bind(request_id.to_string()).bind(digest).bind(encoded)
        .fetch_one(connection).await?;
    Ok(order)
}
