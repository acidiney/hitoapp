use crate::{CuratedWavRecording, policy::Error};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use hmac::{Hmac, Mac};
use sha2::Sha256;
use subtle::ConstantTimeEq;
use uuid::Uuid;
use zeroize::Zeroizing;

pub(crate) struct Receipts {
    key: Zeroizing<[u8; 32]>,
    authority: [u8; 32],
    catalog: [u8; 32],
}

impl Receipts {
    pub(crate) fn new(key: Zeroizing<[u8; 32]>, authority: [u8; 32], catalog: [u8; 32]) -> Self {
        Self {
            key,
            authority,
            catalog,
        }
    }

    pub(crate) fn same_key(&self, other: &[u8; 32]) -> bool {
        bool::from(self.key.as_ref().ct_eq(other))
    }

    pub(crate) fn issue(
        &self,
        operation: Uuid,
        entry: &CuratedWavRecording,
    ) -> Result<String, Error> {
        let tag = self.mac(operation, entry)?.finalize().into_bytes();
        Ok(format!("p1.{}", URL_SAFE_NO_PAD.encode(tag)))
    }

    pub(crate) fn verify(
        &self,
        operation: Uuid,
        entry: &CuratedWavRecording,
        handle: &str,
    ) -> Result<(), Error> {
        let encoded = handle
            .strip_prefix("p1.")
            .filter(|value| value.len() == 43)
            .ok_or(Error::NotFound)?;
        let tag = URL_SAFE_NO_PAD
            .decode(encoded)
            .map_err(|_| Error::NotFound)?;
        self.mac(operation, entry)?
            .verify_slice(&tag)
            .map_err(|_| Error::NotFound)
    }

    fn mac(&self, operation: Uuid, entry: &CuratedWavRecording) -> Result<Hmac<Sha256>, Error> {
        let mut mac =
            Hmac::<Sha256>::new_from_slice(self.key.as_ref()).map_err(|_| Error::Unavailable)?;
        mac.update(b"org.hito.premiumize.preparation.v1\0");
        mac.update(&self.authority);
        mac.update(&self.catalog);
        mac.update(operation.as_bytes());
        let canonical = serde_json::to_vec(&(
            &entry.recording,
            &entry.source_id,
            &entry.label,
            &entry.binding,
        ))
        .map_err(|_| Error::Unavailable)?;
        mac.update(&(canonical.len() as u64).to_be_bytes());
        mac.update(&canonical);
        Ok(mac)
    }
}
