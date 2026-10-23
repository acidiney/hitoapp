use super::{Collection, SyncEvent, SyncPage};
use crate::{Actor, Error, Result};
use sqlx::{Sqlite, Transaction};

const MAX_SYNC_PAGE_BYTES: usize = 16 * 1024 * 1024;
pub(super) const MAX_SYNC_PAYLOAD_BYTES: usize = MAX_SYNC_PAGE_BYTES - 512;
const PAGE_FRAME_BYTES: usize = 128;

impl Collection {
    #[tracing::instrument(level = "debug", skip_all, err(Debug), fields(account_id = actor.id(), cursor = after))]
    pub async fn events(&self, actor: &Actor, after: i64) -> Result<SyncPage> {
        if after < 0 {
            return Err(Error::Invalid("Cursor must not be negative".into()));
        }
        actor.check_session(&self.store).await?;
        let mut tx = self.store.pool.begin().await?;
        let mut cursor = after;
        let mut operations = Vec::new();
        let mut bytes = PAGE_FRAME_BYTES;
        let mut has_more = false;
        loop {
            let next: Option<(i64, String, Option<String>, i64)> = sqlx::query_as(
                "SELECT sequence, origin, device_id, length(CAST(payload AS BLOB))
                 FROM sync_events WHERE user_id = ? AND sequence > ? ORDER BY sequence LIMIT 1",
            )
            .bind(actor.id())
            .bind(cursor)
            .fetch_optional(&mut *tx)
            .await?;
            let Some((sequence, origin, device, length)) = next else {
                break;
            };
            let length = usize::try_from(length)
                .map_err(|_| Error::Internal("Stored sync event has an invalid length".into()))?;
            if length > MAX_SYNC_PAYLOAD_BYTES {
                return Err(Error::Internal(
                    "Stored sync event exceeds the size limit".into(),
                ));
            }
            if operations.len() == 200 || length > MAX_SYNC_PAGE_BYTES - bytes {
                has_more = true;
                break;
            }
            let event = read(&mut tx, actor.id(), sequence, &origin, device).await?;
            let encoded = serde_json::to_vec(&event)
                .map_err(|_| Error::Internal("Sync event could not be encoded".into()))?;
            if encoded.len() + 1 > MAX_SYNC_PAGE_BYTES - bytes {
                if operations.is_empty() {
                    return Err(Error::Internal(
                        "Stored sync event cannot fit a page".into(),
                    ));
                }
                has_more = true;
                break;
            }
            bytes += encoded.len() + 1;
            operations.push(event);
            cursor = sequence;
        }
        tx.commit().await?;
        actor.check_session(&self.store).await?;
        tracing::debug!(events = operations.len(), bytes, "Sync events read");
        Ok(SyncPage {
            operations,
            cursor,
            has_more,
        })
    }
}

async fn read(
    tx: &mut Transaction<'_, Sqlite>,
    account: &str,
    sequence: i64,
    origin: &str,
    device: Option<String>,
) -> Result<SyncEvent> {
    let payload: String =
        sqlx::query_scalar("SELECT payload FROM sync_events WHERE user_id = ? AND sequence = ?")
            .bind(account)
            .bind(sequence)
            .fetch_one(&mut **tx)
            .await?;
    match (origin, device) {
        ("client_operation", Some(device_id)) => Ok(SyncEvent::Operation {
            sequence,
            device_id,
            operation: serde_json::from_str(&payload)
                .map_err(|_| Error::Internal("Stored sync operation is invalid".into()))?,
        }),
        ("playlist_import" | "playlist_follow" | "playlist_metadata", None) => {
            Ok(SyncEvent::Publication {
                sequence,
                publication: serde_json::from_str(&payload).map_err(|_| {
                    Error::Internal("Stored collection publication is invalid".into())
                })?,
            })
        }
        _ => Err(Error::Internal(
            "Stored sync event has an invalid origin".into(),
        )),
    }
}
