use std::sync::Arc;

use rand::{RngCore, rngs::OsRng};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::{Error, storage::Storage, wire::ReporterCredential};

#[derive(Clone)]
pub(crate) struct Admission {
    storage: Arc<Storage>,
    admin_hash: [u8; 32],
}

pub(crate) struct Reporter(pub(crate) Uuid);
pub(crate) struct Administrator;

impl Admission {
    pub(crate) fn new(storage: Arc<Storage>, admin_hash: [u8; 32]) -> Self {
        Self {
            storage,
            admin_hash,
        }
    }

    pub(crate) fn administrator(&self, token: &str) -> Result<Administrator, Error> {
        let supplied: [u8; 32] = Sha256::digest(token.as_bytes()).into();
        if supplied != self.admin_hash {
            return Err(Error::Unauthorized);
        }
        Ok(Administrator)
    }

    pub(crate) async fn reporter(&self, token: &str) -> Result<Reporter, Error> {
        let hash = Sha256::digest(token.as_bytes()).to_vec();
        let id: Option<String> =
            sqlx::query_scalar("SELECT id FROM reporters WHERE token_hash = ?")
                .bind(hash)
                .fetch_optional(&self.storage.pool)
                .await?;
        let id = id.ok_or(Error::Unauthorized)?;
        Ok(Reporter(
            Uuid::parse_str(&id).map_err(|_| Error::StorageUnavailable)?,
        ))
    }

    pub(crate) async fn provision(&self, _: Administrator) -> Result<ReporterCredential, Error> {
        let reporter_id = Uuid::new_v4();
        let mut bytes = [0_u8; 32];
        OsRng.fill_bytes(&mut bytes);
        let token = format!("{:x}", Sha256::digest(bytes));
        sqlx::query("INSERT INTO reporters (id, token_hash) VALUES (?, ?)")
            .bind(reporter_id.to_string())
            .bind(Sha256::digest(token.as_bytes()).to_vec())
            .execute(&self.storage.pool)
            .await?;
        tracing::debug!(
            event = "community_reporter_provisioned",
            outcome = "created"
        );
        Ok(ReporterCredential { reporter_id, token })
    }
}
