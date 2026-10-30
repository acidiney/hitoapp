use super::{ArtistAssociation, authority::Credit, provider::Connection};
use crate::{Actor, Error, Result};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use chacha20poly1305::{
    XChaCha20Poly1305, XNonce,
    aead::{Aead, AeadCore, KeyInit, OsRng, Payload},
};
use hito_plugin_sdk::{ArtistDiscographyRequest, ReleaseGroupEditionsRequest};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use sha2::{Digest, Sha256};
use std::{io, sync::Arc};

const DOMAIN: &[u8] = b"hito.artist-catalog.discography.continuation.v1";
const PREFIX: &str = "hito-artist-discography-v1.";
const EDITIONS_DOMAIN: &[u8] = b"hito.artist-catalog.editions.continuation.v1";
const EDITIONS_PREFIX: &str = "hito-artist-editions-v1.";
const MAX_PAYLOAD_BYTES: usize = 6_000;
const MAX_DECODED_BYTES: usize = 6_144;
pub(super) const MAX_TOKEN_BYTES: usize = 8_192;

#[derive(Clone)]
pub(super) struct Key(Arc<XChaCha20Poly1305>);

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Page<T> {
    binding: [u8; 32],
    request: T,
}

impl Key {
    pub(super) fn new() -> Self {
        let key = XChaCha20Poly1305::generate_key(&mut OsRng);
        Self(Arc::new(XChaCha20Poly1305::new(&key)))
    }

    pub(super) fn seal(
        &self,
        binding: [u8; 32],
        request: ArtistDiscographyRequest,
    ) -> Result<String> {
        request.validate().map_err(|_| invalid())?;
        if request.cursor.is_none() {
            return Err(invalid());
        }
        self.seal_page(DOMAIN, PREFIX, binding, request)
    }

    pub(super) fn open(
        &self,
        token: &str,
        binding: [u8; 32],
        limit: u16,
    ) -> Result<ArtistDiscographyRequest> {
        let page: Page<ArtistDiscographyRequest> = self.open_page(DOMAIN, PREFIX, token)?;
        page.request.validate().map_err(|_| invalid())?;
        if page.request.cursor.is_none() {
            return Err(invalid());
        }
        if page.binding != binding || page.request.limit != limit {
            return Err(Error::Conflict(
                "The catalog continuation changed. Start again.".into(),
            ));
        }
        Ok(page.request)
    }

    pub(super) fn seal_editions(
        &self,
        binding: [u8; 32],
        request: ReleaseGroupEditionsRequest,
    ) -> Result<String> {
        request.validate().map_err(|_| invalid())?;
        if request.cursor.is_none() {
            return Err(invalid());
        }
        self.seal_page(EDITIONS_DOMAIN, EDITIONS_PREFIX, binding, request)
    }

    pub(super) fn open_editions(
        &self,
        token: &str,
        binding: [u8; 32],
        release_group_id: &str,
        limit: u16,
    ) -> Result<ReleaseGroupEditionsRequest> {
        let page: Page<ReleaseGroupEditionsRequest> =
            self.open_page(EDITIONS_DOMAIN, EDITIONS_PREFIX, token)?;
        page.request.validate().map_err(|_| invalid())?;
        if page.request.cursor.is_none() {
            return Err(invalid());
        }
        if page.binding != binding
            || page.request.release_group_id != release_group_id
            || page.request.limit != limit
        {
            return Err(Error::Conflict(
                "The catalog continuation changed. Start again.".into(),
            ));
        }
        Ok(page.request)
    }

    fn seal_page<T: Serialize>(
        &self,
        domain: &[u8],
        prefix: &str,
        binding: [u8; 32],
        request: T,
    ) -> Result<String> {
        let mut bytes = [0_u8; MAX_PAYLOAD_BYTES];
        let length = {
            let mut output = io::Cursor::new(bytes.as_mut_slice());
            serde_json::to_writer(&mut output, &Page { binding, request }).map_err(|_| {
                Error::Internal("The catalog continuation could not be encoded".into())
            })?;
            output.position() as usize
        };
        let nonce = XChaCha20Poly1305::generate_nonce(&mut OsRng);
        let ciphertext = self
            .0
            .encrypt(
                &nonce,
                Payload {
                    msg: &bytes[..length],
                    aad: domain,
                },
            )
            .map_err(|_| {
                Error::Internal("The catalog continuation could not be protected".into())
            })?;
        let mut envelope = Vec::with_capacity(nonce.len() + ciphertext.len());
        envelope.extend_from_slice(&nonce);
        envelope.extend_from_slice(&ciphertext);
        let token = format!("{prefix}{}", URL_SAFE_NO_PAD.encode(envelope));
        if token.len() > MAX_TOKEN_BYTES {
            return Err(Error::Internal(
                "The catalog continuation exceeds its size limit".into(),
            ));
        }
        Ok(token)
    }

    fn open_page<T: DeserializeOwned>(
        &self,
        domain: &[u8],
        prefix: &str,
        token: &str,
    ) -> Result<Page<T>> {
        if token.len() > MAX_TOKEN_BYTES || !token.is_ascii() {
            return Err(invalid());
        }
        let encoded = token.strip_prefix(prefix).ok_or_else(invalid)?;
        let mut bytes = [0_u8; MAX_DECODED_BYTES];
        let length = URL_SAFE_NO_PAD
            .decode_slice(encoded, &mut bytes)
            .map_err(|_| invalid())?;
        if length <= 40 || length > MAX_PAYLOAD_BYTES + 40 {
            return Err(invalid());
        }
        let plaintext = self
            .0
            .decrypt(
                XNonce::from_slice(&bytes[..24]),
                Payload {
                    msg: &bytes[24..length],
                    aad: domain,
                },
            )
            .map_err(|_| invalid())?;
        serde_json::from_slice(&plaintext).map_err(|_| invalid())
    }
}

pub(super) fn binding(
    actor: &Actor,
    association: &ArtistAssociation,
    credit: &Credit,
    policy_revision: i64,
    connection: &Connection,
) -> Result<[u8; 32]> {
    let mut output = Fingerprint {
        digest: Sha256::new(),
        remaining: 16_384,
    };
    output.digest.update(DOMAIN);
    serde_json::to_writer(
        &mut output,
        &(
            actor.id(),
            actor.session_binding(),
            association,
            credit,
            policy_revision,
            connection.review_connection(),
        ),
    )
    .map_err(|_| {
        Error::Internal("The catalog continuation context exceeds its size limit".into())
    })?;
    Ok(output.digest.finalize().into())
}

struct Fingerprint {
    digest: Sha256,
    remaining: usize,
}

impl io::Write for Fingerprint {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() > self.remaining {
            return Err(io::ErrorKind::InvalidData.into());
        }
        self.remaining -= bytes.len();
        self.digest.update(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn invalid() -> Error {
    Error::Invalid("Use a valid catalog continuation or start again".into())
}
