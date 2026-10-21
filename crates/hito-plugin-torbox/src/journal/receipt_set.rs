use super::records::{MAX_RECEIPTS, WriteError};
use crate::StateError;
use sha2::{Digest, Sha256};
use sqlx::{Row, SqliteConnection};
use uuid::Uuid;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) struct ReceiptSet {
    pub(super) count: u64,
    pub(super) digest: [u8; 32],
}

pub(super) fn empty(state: Uuid) -> ReceiptSet {
    finish(start(state), 0)
}

pub(super) async fn read(
    connection: &mut SqliteConnection,
    state: Uuid,
) -> Result<ReceiptSet, WriteError> {
    let rows = sqlx::query("SELECT operation_id, mac FROM operations INDEXED BY operation_receipt_macs ORDER BY operation_id LIMIT 1025")
        .fetch_all(connection).await.map_err(WriteError::Database)?;
    if rows.len() > MAX_RECEIPTS {
        return Err(StateError::InvalidState.into());
    }
    let count = rows.len() as u64;
    let mut hash = start(state);
    let mut previous = None;
    for row in rows {
        let operation: [u8; 16] = row
            .try_get::<Vec<u8>, _>(0)
            .map_err(WriteError::Database)?
            .try_into()
            .map_err(|_| StateError::InvalidState)?;
        let mac: [u8; 32] = row
            .try_get::<Vec<u8>, _>(1)
            .map_err(WriteError::Database)?
            .try_into()
            .map_err(|_| StateError::InvalidState)?;
        if Uuid::from_bytes(operation).is_nil() || previous.is_some_and(|old| old >= operation) {
            return Err(StateError::InvalidState.into());
        }
        hash.update(operation);
        hash.update(mac);
        previous = Some(operation);
    }
    Ok(finish(hash, count))
}

fn start(state: Uuid) -> Sha256 {
    let mut hash = Sha256::new();
    hash.update(b"hito.torbox.journal.receipt-set.v4\0");
    hash.update(state.as_bytes());
    hash
}

fn finish(mut hash: Sha256, count: u64) -> ReceiptSet {
    hash.update(count.to_be_bytes());
    ReceiptSet {
        count,
        digest: hash.finalize().into(),
    }
}
