use super::{Collection, SyncEvent, SyncPage};
use crate::{Actor, Error, Result};

impl Collection {
    #[tracing::instrument(level = "debug", skip_all, err(Debug), fields(account_id = actor.id(), cursor = after))]
    pub async fn events(&self, actor: &Actor, after: i64) -> Result<SyncPage> {
        if after < 0 {
            return Err(Error::Invalid("Cursor must not be negative".into()));
        }
        let mut rows: Vec<(i64, String, String)> = sqlx::query_as(
            "SELECT sequence, device_id, payload FROM sync_operations
             WHERE user_id = ? AND sequence > ? ORDER BY sequence LIMIT 201",
        )
        .bind(actor.id())
        .bind(after)
        .fetch_all(&self.store.pool)
        .await?;
        let has_more = rows.len() > 200;
        rows.truncate(200);
        let operations: Vec<_> = rows
            .into_iter()
            .map(|(sequence, device_id, payload)| {
                Ok(SyncEvent {
                    sequence,
                    device_id,
                    operation: serde_json::from_str(&payload)
                        .map_err(|e| Error::Internal(e.to_string()))?,
                })
            })
            .collect::<Result<_>>()?;
        let cursor = operations.last().map(|op| op.sequence).unwrap_or(after);
        Ok(SyncPage {
            operations,
            cursor,
            has_more,
        })
    }
}
