use crate::{InitError, http::Error};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use chacha20poly1305::{
    ChaCha20Poly1305, KeyInit, Nonce,
    aead::{Aead, Payload},
};
use rand::RngCore;
use subtle::ConstantTimeEq;
use uuid::Uuid;
use zeroize::Zeroizing;

const DOMAIN: &[u8] = b"hito.all-debrid.preparation.v1";

pub(crate) struct ReceiptKey(Zeroizing<[u8; 32]>);
pub(crate) struct Ticket {
    pub(crate) kind: u8,
    pub(crate) delayed: u64,
}

impl ReceiptKey {
    pub(crate) fn new(key: [u8; 32]) -> Result<Self, InitError> {
        let key = Zeroizing::new(key);
        if key.iter().all(|byte| *byte == 0) {
            return Err(InitError::InvalidReceiptKey);
        }
        Ok(Self(key))
    }

    pub(crate) fn encode(
        &self,
        operation: Uuid,
        witness: &[u8; 32],
        ticket: Ticket,
    ) -> Result<String, Error> {
        let mut plain = Zeroizing::new([0_u8; 58]);
        plain[0] = 1;
        plain[1..17].copy_from_slice(operation.as_bytes());
        plain[17..49].copy_from_slice(witness);
        plain[49] = ticket.kind;
        plain[50..58].copy_from_slice(&ticket.delayed.to_be_bytes());
        let mut nonce = [0_u8; 12];
        rand::rngs::OsRng
            .try_fill_bytes(&mut nonce)
            .map_err(|_| Error::Unavailable)?;
        let cipher =
            ChaCha20Poly1305::new_from_slice(self.0.as_ref()).map_err(|_| Error::Unavailable)?;
        let encrypted = cipher
            .encrypt(
                Nonce::from_slice(&nonce),
                Payload {
                    msg: plain.as_ref(),
                    aad: DOMAIN,
                },
            )
            .map_err(|_| Error::Unavailable)?;
        let mut sealed = nonce.to_vec();
        sealed.extend_from_slice(&encrypted);
        Ok(URL_SAFE_NO_PAD.encode(sealed))
    }

    pub(crate) fn decode(
        &self,
        handle: &str,
        operation: Uuid,
        witness: &[u8; 32],
    ) -> Result<Ticket, Error> {
        if handle.len() > 2048 {
            return Err(Error::NotFound);
        }
        let bytes = URL_SAFE_NO_PAD
            .decode(handle)
            .map_err(|_| Error::NotFound)?;
        if bytes.len() != 86 {
            return Err(Error::NotFound);
        }
        let cipher =
            ChaCha20Poly1305::new_from_slice(self.0.as_ref()).map_err(|_| Error::Unavailable)?;
        let plain = Zeroizing::new(
            cipher
                .decrypt(
                    Nonce::from_slice(&bytes[..12]),
                    Payload {
                        msg: &bytes[12..],
                        aad: DOMAIN,
                    },
                )
                .map_err(|_| Error::NotFound)?,
        );
        if plain.len() != 58
            || plain[0] != 1
            || plain[1..17] != *operation.as_bytes()
            || !bool::from(plain[17..49].ct_eq(witness))
        {
            return Err(Error::NotFound);
        }
        let delayed = u64::from_be_bytes(plain[50..58].try_into().map_err(|_| Error::NotFound)?);
        let kind = plain[49];
        if !matches!((kind, delayed), (1, 1..) | (2, 0)) {
            return Err(Error::NotFound);
        }
        Ok(Ticket { kind, delayed })
    }
}
