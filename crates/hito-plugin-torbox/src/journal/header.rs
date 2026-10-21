use super::{
    database_error,
    receipt_set::{self, ReceiptSet},
    records::WriteError,
};
use crate::{StateError, catalog::Catalog};
use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};
use sqlx::{Connection, Row, SqliteConnection};
use subtle::ConstantTimeEq;
use uuid::Uuid;
use zeroize::{Zeroize, Zeroizing};

const APPLICATION_ID: i64 = 0x48544258;
const VERSION: i64 = 4;
const SCHEMA: &str = include_str!("schema.sql");

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) struct Header {
    pub(super) state_id: Uuid,
    authority: [u8; 32],
    witness: [u8; 32],
    receipts: ReceiptSet,
}

pub(super) async fn create_header(
    connection: &mut SqliteConnection,
    key: &[u8; 32],
    authority: &[u8; 32],
    witness: &[u8; 32],
) -> Result<Header, StateError> {
    let state_id = Uuid::new_v4();
    let header = Header {
        state_id,
        authority: *authority,
        witness: *witness,
        receipts: receipt_set::empty(state_id),
    };
    let mac = header.mac(key)?.finalize().into_bytes();
    let mut transaction = connection.begin().await.map_err(database_error)?;
    for statement in SCHEMA
        .split(';')
        .map(str::trim)
        .filter(|statement| !statement.is_empty())
    {
        sqlx::query(statement)
            .execute(&mut *transaction)
            .await
            .map_err(database_error)?;
    }
    sqlx::query("INSERT INTO bootstrap_metadata VALUES (1, 4, ?, ?, ?, ?, ?, ?)")
        .bind(state_id.as_bytes().as_slice())
        .bind(authority.as_slice())
        .bind(witness.as_slice())
        .bind(header.receipts.count.to_be_bytes().to_vec())
        .bind(header.receipts.digest.as_slice())
        .bind(mac.as_slice())
        .execute(&mut *transaction)
        .await
        .map_err(database_error)?;
    sqlx::query("PRAGMA application_id = 1213481560")
        .execute(&mut *transaction)
        .await
        .map_err(database_error)?;
    sqlx::query("PRAGMA user_version = 4")
        .execute(&mut *transaction)
        .await
        .map_err(database_error)?;
    transaction.commit().await.map_err(database_error)?;
    Ok(header)
}

pub(super) async fn verify_header(
    connection: &mut SqliteConnection,
    key: &[u8; 32],
    authority: &[u8; 32],
) -> Result<Header, StateError> {
    let application: i64 = sqlx::query_scalar("PRAGMA application_id")
        .fetch_one(&mut *connection)
        .await
        .map_err(database_error)?;
    let version: i64 = sqlx::query_scalar("PRAGMA user_version")
        .fetch_one(&mut *connection)
        .await
        .map_err(database_error)?;
    let rows = sqlx::query("SELECT type, name, tbl_name, sql FROM sqlite_schema LIMIT 7")
        .fetch_all(&mut *connection)
        .await
        .map_err(database_error)?;
    if rows.len() != 6 {
        return Err(StateError::InvalidState);
    }
    let mut schema = Vec::new();
    let mut implicit = Vec::new();
    for row in rows {
        let kind: String = row.try_get(0).map_err(database_error)?;
        let name: String = row.try_get(1).map_err(database_error)?;
        let table: String = row.try_get(2).map_err(database_error)?;
        let sql: Option<String> = row.try_get(3).map_err(database_error)?;
        match (kind.as_str(), name.as_str(), table.as_str(), sql) {
            ("table", "bootstrap_metadata", "bootstrap_metadata", Some(sql))
            | ("table", "operations", "operations", Some(sql))
            | (
                "index",
                "current_source_operation" | "operation_receipt_macs",
                "operations",
                Some(sql),
            ) => schema.push(sql),
            (
                "index",
                "sqlite_autoindex_operations_1" | "sqlite_autoindex_operations_2",
                "operations",
                None,
            ) => implicit.push(name.clone()),
            _ => return Err(StateError::InvalidState),
        }
    }
    let mut expected: Vec<&str> = SCHEMA
        .split(';')
        .map(str::trim)
        .filter(|statement| !statement.is_empty())
        .collect();
    schema.sort();
    implicit.sort();
    expected.sort();
    let checked: String = sqlx::query_scalar("PRAGMA quick_check(1)")
        .fetch_one(&mut *connection)
        .await
        .map_err(database_error)?;
    if application != APPLICATION_ID
        || version != VERSION
        || schema.iter().map(String::as_str).collect::<Vec<_>>() != expected
        || implicit.len() != 2
        || implicit[0] != "sqlite_autoindex_operations_1"
        || implicit[1] != "sqlite_autoindex_operations_2"
        || checked != "ok"
    {
        return Err(StateError::InvalidState);
    }
    let header = read_metadata(connection, key, authority).await?;
    header.verify_set(connection).await?;
    Ok(header)
}

async fn read_metadata(
    connection: &mut SqliteConnection,
    key: &[u8; 32],
    authority: &[u8; 32],
) -> Result<Header, StateError> {
    let row = sqlx::query("SELECT state_id, provider_authority, catalog_witness, receipt_count, receipt_digest, mac FROM bootstrap_metadata WHERE singleton = 1 AND version = 4 AND length(state_id) = 16 AND length(provider_authority) = 32 AND length(catalog_witness) = 32 AND length(receipt_count) = 8 AND length(receipt_digest) = 32 AND length(mac) = 32")
        .fetch_one(&mut *connection).await.map_err(database_error)?;
    let state: Vec<u8> = row.try_get(0).map_err(database_error)?;
    let state = Uuid::from_slice(&state).map_err(|_| StateError::InvalidState)?;
    if state.get_version() != Some(uuid::Version::Random) {
        return Err(StateError::InvalidState);
    }
    let saved_authority: [u8; 32] = row
        .try_get::<Vec<u8>, _>(1)
        .map_err(database_error)?
        .try_into()
        .map_err(|_| StateError::InvalidState)?;
    let saved_witness: [u8; 32] = row
        .try_get::<Vec<u8>, _>(2)
        .map_err(database_error)?
        .try_into()
        .map_err(|_| StateError::InvalidState)?;
    let count: [u8; 8] = row
        .try_get::<Vec<u8>, _>(3)
        .map_err(database_error)?
        .try_into()
        .map_err(|_| StateError::InvalidState)?;
    let count = u64::from_be_bytes(count);
    let digest: [u8; 32] = row
        .try_get::<Vec<u8>, _>(4)
        .map_err(database_error)?
        .try_into()
        .map_err(|_| StateError::InvalidState)?;
    if count > super::records::MAX_RECEIPTS as u64 {
        return Err(StateError::InvalidState);
    }
    let header = Header {
        state_id: state,
        authority: saved_authority,
        witness: saved_witness,
        receipts: ReceiptSet { count, digest },
    };
    let mac: Vec<u8> = row.try_get(5).map_err(database_error)?;
    header
        .mac(key)?
        .verify_slice(&mac)
        .map_err(|_| StateError::InvalidState)?;
    if !bool::from(saved_authority.ct_eq(authority)) {
        return Err(StateError::AuthorityChanged);
    }
    Ok(header)
}

impl Header {
    pub(super) async fn verify(
        &self,
        connection: &mut SqliteConnection,
        key: &[u8; 32],
    ) -> Result<(), StateError> {
        if read_metadata(connection, key, &self.authority).await? != *self {
            return Err(StateError::InvalidState);
        }
        self.verify_set(connection).await
    }

    async fn verify_set(&self, connection: &mut SqliteConnection) -> Result<(), StateError> {
        if receipt_set::read(connection, self.state_id).await? != self.receipts {
            return Err(StateError::InvalidState);
        }
        Ok(())
    }

    pub(super) async fn refresh(
        &self,
        connection: &mut SqliteConnection,
        key: &[u8; 32],
    ) -> Result<Self, WriteError> {
        let next = Self {
            receipts: receipt_set::read(connection, self.state_id).await?,
            ..*self
        };
        let previous_mac = self.mac(key)?.finalize().into_bytes();
        let mac = next.mac(key)?.finalize().into_bytes();
        let changed = sqlx::query("UPDATE bootstrap_metadata SET receipt_count = ?, receipt_digest = ?, mac = ? WHERE singleton = 1 AND mac = ?")
            .bind(next.receipts.count.to_be_bytes().to_vec())
            .bind(next.receipts.digest.as_slice()).bind(mac.as_slice()).bind(previous_mac.as_slice())
            .execute(connection).await.map_err(WriteError::Database)?.rows_affected();
        if changed != 1 {
            return Err(StateError::InvalidState.into());
        }
        Ok(next)
    }

    fn mac(&self, key: &[u8; 32]) -> Result<Hmac<Sha256>, StateError> {
        let mut mac =
            <Hmac<Sha256> as Mac>::new_from_slice(key).map_err(|_| StateError::Unavailable)?;
        mac.update(b"hito.torbox.journal.header.v4");
        mac.update(self.state_id.as_bytes());
        mac.update(&VERSION.to_be_bytes());
        mac.update(&4096_u64.to_be_bytes());
        mac.update(&4096_u64.to_be_bytes());
        mac.update(&1024_u64.to_be_bytes());
        mac.update(&self.authority);
        mac.update(&self.witness);
        mac.update(&self.receipts.count.to_be_bytes());
        mac.update(&self.receipts.digest);
        Ok(mac)
    }
}

pub(super) fn catalog_witness(catalog: &Catalog) -> [u8; 32] {
    let mut hash = Sha256::new();
    hash.update(b"hito.torbox.journal.catalog.v1");
    for index in 0..catalog.len() {
        let entry = catalog.entry(index);
        for field in [
            entry.recording.id.as_bytes(),
            entry.source_id.as_bytes(),
            entry.binding.info_hash.as_bytes(),
            entry.binding.file_name.as_bytes(),
        ] {
            hash.update((field.len() as u64).to_be_bytes());
            hash.update(field);
        }
        for field in [
            entry.binding.torrent_id,
            entry.binding.torrent_bytes,
            entry.binding.file_id,
            entry.binding.file_bytes,
        ] {
            hash.update(field.to_be_bytes());
        }
    }
    hash.finalize().into()
}

pub(super) fn integrity_key(key: &[u8; 32]) -> Result<Zeroizing<[u8; 32]>, StateError> {
    let mut mac =
        <Hmac<Sha256> as Mac>::new_from_slice(key).map_err(|_| StateError::Unavailable)?;
    mac.update(b"hito.torbox.journal.integrity-key.v4");
    let mut digest = mac.finalize().into_bytes();
    let mut derived = Zeroizing::new([0_u8; 32]);
    derived.copy_from_slice(&digest);
    digest[..].zeroize();
    Ok(derived)
}
