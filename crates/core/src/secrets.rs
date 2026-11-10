mod key_file;
#[cfg(test)]
mod replica_work;
#[cfg(test)]
mod startup_work;

use crate::{Error, Result, store::Store};
use chacha20poly1305::{
    XChaCha20Poly1305, XNonce,
    aead::{Aead, AeadCore, KeyInit, OsRng, Payload},
};
use sha2::{Digest, Sha256};
use std::{path::Path, sync::Arc};
use zeroize::Zeroizing;

#[derive(Clone)]
pub(super) struct Vault(Arc<Option<Key>>);

struct Key {
    cipher: XChaCha20Poly1305,
    fingerprint: [u8; 32],
}

impl Vault {
    pub(super) async fn open(store: &Store, database: &Path) -> Result<Self> {
        let mut fingerprints: Vec<Vec<u8>> = sqlx::query_scalar(
            "SELECT substr(credential,1,33) FROM plugin_installations INDEXED BY plugin_credential_fingerprints
            WHERE credential IS NOT NULL ORDER BY substr(credential,1,33) ASC LIMIT 1",
        )
        .fetch_all(&store.pool)
        .await?;
        let last: Option<Vec<u8>> = sqlx::query_scalar(
            "SELECT substr(credential,1,33) FROM plugin_installations INDEXED BY plugin_credential_fingerprints
            WHERE credential IS NOT NULL ORDER BY substr(credential,1,33) DESC LIMIT 1",
        )
        .fetch_optional(&store.pool)
        .await?;
        fingerprints.extend(last);
        let pending: Vec<Vec<u8>> = sqlx::query_scalar(
            "SELECT DISTINCT substr(pending_secret,1,33) FROM plugin_authorization_attempts INDEXED BY expired_plugin_authorization_attempts
            WHERE expires_at > unixepoch() AND status IN ('pending','exchanging') LIMIT 2",
        )
        .fetch_all(&store.pool)
        .await?;
        fingerprints.extend(pending);
        let connections: Vec<Vec<u8>> = sqlx::query_scalar(
            "SELECT DISTINCT substr(tokens,1,33) FROM plugin_connections INDEXED BY plugin_connection_fingerprints
            WHERE tokens IS NOT NULL LIMIT 2",
        )
        .fetch_all(&store.pool)
        .await?;
        fingerprints.extend(connections);
        let community: Option<Vec<u8>> = sqlx::query_scalar(
            "SELECT substr(credential,1,33) FROM community_reporters
             INDEXED BY community_credential_fingerprints ORDER BY substr(credential,1,33) ASC LIMIT 1",
        )
        .fetch_optional(&store.pool)
        .await?;
        fingerprints.extend(community);
        let community: Option<Vec<u8>> = sqlx::query_scalar(
            "SELECT substr(credential,1,33) FROM community_reporters
             INDEXED BY community_credential_fingerprints ORDER BY substr(credential,1,33) DESC LIMIT 1",
        )
        .fetch_optional(&store.pool)
        .await?;
        fingerprints.extend(community);
        let replica: Option<Vec<u8>> = sqlx::query_scalar(
            "SELECT substr(credential,1,33) FROM replica_connections
             INDEXED BY replica_credential_fingerprints WHERE credential IS NOT NULL
             ORDER BY substr(credential,1,33) ASC LIMIT 1",
        )
        .fetch_optional(&store.pool)
        .await?;
        fingerprints.extend(replica);
        let replica: Option<Vec<u8>> = sqlx::query_scalar(
            "SELECT substr(credential,1,33) FROM replica_connections
             INDEXED BY replica_credential_fingerprints WHERE credential IS NOT NULL
             ORDER BY substr(credential,1,33) DESC LIMIT 1",
        )
        .fetch_optional(&store.pool)
        .await?;
        fingerprints.extend(replica);
        let path = database.with_extension("plugin-key");
        let create = fingerprints.is_empty();
        let loaded = tokio::task::spawn_blocking(move || key_file::load(&path, create)).await;
        let key = loaded
            .ok()
            .and_then(std::result::Result::ok)
            .and_then(|bytes| {
                let fingerprint: [u8; 32] = Sha256::digest(bytes.as_ref()).into();
                let mut header = vec![1];
                header.extend_from_slice(&fingerprint);
                if fingerprints.iter().any(|stored| stored != &header) {
                    return None;
                }
                Some(Key {
                    cipher: XChaCha20Poly1305::new(bytes.as_ref().into()),
                    fingerprint,
                })
            });
        if key.is_none() {
            tracing::warn!(
                reason = "key_unavailable",
                "Plugin credential storage is unavailable; local services remain enabled"
            );
        }
        Ok(Self(Arc::new(key)))
    }

    pub(super) fn ready(&self) -> bool {
        self.0.is_some()
    }

    pub(super) fn seal(
        &self,
        account: &str,
        installation: &str,
        service: &str,
        token: &str,
    ) -> Result<Vec<u8>> {
        validate(token)?;
        self.seal_payload(&context(account, installation, service), token.as_bytes())
    }

    pub(super) fn seal_pending_authorization(
        &self,
        account: &str,
        installation: &str,
        service: &str,
        attempt: &str,
        secrets: &[u8],
    ) -> Result<Vec<u8>> {
        if secrets.is_empty() || secrets.len() > 1024 {
            return Err(Error::Internal(
                "Plugin authorization secrets exceed their storage limit".into(),
            ));
        }
        let aad = format!(
            "hito.plugin.authorization.pending.v1|{account}|{installation}|{service}|{attempt}"
        );
        self.seal_payload(&aad, secrets)
    }

    pub(super) fn reveal_pending_authorization(
        &self,
        account: &str,
        installation: &str,
        service: &str,
        attempt: &str,
        envelope: &[u8],
    ) -> Result<Zeroizing<String>> {
        let aad = format!(
            "hito.plugin.authorization.pending.v1|{account}|{installation}|{service}|{attempt}"
        );
        self.reveal_payload(&aad, envelope)
    }

    pub(super) fn seal_connection(
        &self,
        account: &str,
        installation: &str,
        service: &str,
        declaration: &str,
        payload: &[u8],
    ) -> Result<Vec<u8>> {
        if payload.is_empty() || payload.len() > 8400 {
            return Err(Error::Internal(
                "Provider tokens exceed their storage limit".into(),
            ));
        }
        self.seal_payload(
            &connection_context(account, installation, service, declaration),
            payload,
        )
    }

    pub(super) fn reveal_connection(
        &self,
        account: &str,
        installation: &str,
        service: &str,
        declaration: &str,
        envelope: &[u8],
    ) -> Result<Zeroizing<Vec<u8>>> {
        self.open_payload(
            &connection_context(account, installation, service, declaration),
            envelope,
            8800,
        )
    }

    fn seal_payload(&self, aad: &str, plaintext: &[u8]) -> Result<Vec<u8>> {
        let key = self.0.as_ref().as_ref().ok_or_else(unavailable)?;
        let nonce = XChaCha20Poly1305::generate_nonce(&mut OsRng);
        let ciphertext = key
            .cipher
            .encrypt(
                &nonce,
                Payload {
                    msg: plaintext,
                    aad: aad.as_bytes(),
                },
            )
            .map_err(|_| unavailable())?;
        let mut envelope = vec![1];
        envelope.extend_from_slice(&key.fingerprint);
        envelope.extend_from_slice(&nonce);
        envelope.extend(ciphertext);
        Ok(envelope)
    }

    pub(super) fn reveal(
        &self,
        account: &str,
        installation: &str,
        service: &str,
        envelope: &[u8],
    ) -> Result<Zeroizing<String>> {
        self.reveal_payload(&context(account, installation, service), envelope)
    }

    pub(super) fn seal_replica(
        &self,
        owner: &str,
        binding: &str,
        endpoint: &str,
        remote_account: &str,
        token: &str,
    ) -> Result<Vec<u8>> {
        validate(token)?;
        self.seal_payload(
            &replica_context(owner, binding, endpoint, remote_account)?,
            token.as_bytes(),
        )
    }

    pub(super) fn reveal_replica(
        &self,
        owner: &str,
        binding: &str,
        endpoint: &str,
        remote_account: &str,
        envelope: &[u8],
    ) -> Result<Zeroizing<String>> {
        self.reveal_payload(
            &replica_context(owner, binding, endpoint, remote_account)?,
            envelope,
        )
    }

    pub(super) fn seal_community(
        &self,
        account: &str,
        service: &str,
        reporter: &str,
        token: &str,
    ) -> Result<Vec<u8>> {
        validate(token)?;
        self.seal_payload(
            &community_context(account, service, reporter),
            token.as_bytes(),
        )
    }

    pub(super) fn reveal_community(
        &self,
        account: &str,
        service: &str,
        reporter: &str,
        envelope: &[u8],
    ) -> Result<Zeroizing<String>> {
        self.reveal_payload(&community_context(account, service, reporter), envelope)
    }

    fn reveal_payload(&self, aad: &str, envelope: &[u8]) -> Result<Zeroizing<String>> {
        let plaintext = self.open_payload(aad, envelope, 4169)?;
        let token =
            Zeroizing::new(String::from_utf8(plaintext.to_vec()).map_err(|_| unavailable())?);
        validate(&token)?;
        Ok(token)
    }

    fn open_payload(
        &self,
        aad: &str,
        envelope: &[u8],
        maximum: usize,
    ) -> Result<Zeroizing<Vec<u8>>> {
        let key = self.0.as_ref().as_ref().ok_or_else(unavailable)?;
        if !(73..=maximum).contains(&envelope.len())
            || envelope[0] != 1
            || envelope[1..33] != key.fingerprint
        {
            return Err(unavailable());
        }
        Ok(Zeroizing::new(
            key.cipher
                .decrypt(
                    XNonce::from_slice(&envelope[33..57]),
                    Payload {
                        msg: &envelope[57..],
                        aad: aad.as_bytes(),
                    },
                )
                .map_err(|_| unavailable())?,
        ))
    }
}

pub(super) fn validate(token: &str) -> Result<()> {
    if token.is_empty() || token.len() > 4096 || !token.bytes().all(|byte| byte.is_ascii_graphic())
    {
        return Err(Error::Invalid(
            "Use a bearer token of 1 to 4096 visible ASCII characters".into(),
        ));
    }
    Ok(())
}

fn context(account: &str, installation: &str, service: &str) -> String {
    format!("hito.plugin.credential.v1|{account}|{installation}|{service}")
}

fn connection_context(
    account: &str,
    installation: &str,
    service: &str,
    declaration: &str,
) -> String {
    format!("hito.plugin.connection.v1|{account}|{installation}|{service}|{declaration}")
}

fn community_context(account: &str, service: &str, reporter: &str) -> String {
    format!("hito.community.reporter.v1|{account}|{service}|{reporter}")
}

fn unavailable() -> Error {
    Error::Conflict("Plugin credentials cannot be read. Restore the server's plugin key and restart, or clear the saved credential.".into())
}

fn replica_context(
    owner: &str,
    binding: &str,
    endpoint: &str,
    remote_account: &str,
) -> Result<String> {
    serde_json::to_string(&(
        "hito.replica.session.v1",
        owner,
        binding,
        endpoint,
        remote_account,
    ))
    .map_err(|_| unavailable())
}
