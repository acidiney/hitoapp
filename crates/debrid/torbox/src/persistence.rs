use crate::{
    GeneratedFileCandidate, TorBoxClient, TorrentFileBinding, transport::PROVIDER_ORIGIN,
    workflow::WorkflowLog,
};
use chacha20poly1305::{
    ChaCha20Poly1305, KeyInit, Nonce,
    aead::{Aead, Payload},
};
use hmac::{Hmac, Mac};
use rand::RngCore;
use sha2::Sha256;
use std::time::Instant;
use uuid::Uuid;
use zeroize::{Zeroize, Zeroizing};

const VERSION: u8 = 1;
const NONCE_BYTES: usize = 12;
const DOMAIN: &[u8] = b"hito.torbox.candidate.v1";

pub struct CandidateKey(Zeroizing<[u8; 32]>);

pub struct CandidateContext<'a> {
    pub state_id: Uuid,
    pub operation_id: Uuid,
    pub handle: Uuid,
    pub recording_id: &'a str,
    pub source_id: &'a str,
    pub binding: &'a TorrentFileBinding,
}

pub struct CandidateAuthority([u8; 32]);

pub struct SealedCandidate(Vec<u8>);

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum PersistenceError {
    #[error("The private candidate key is invalid")]
    InvalidKey,
    #[error("The private candidate state is invalid")]
    Rejected,
    #[error("Private candidate persistence is unavailable")]
    Unavailable,
}

impl CandidateKey {
    pub fn new(bytes: [u8; 32]) -> Result<Self, PersistenceError> {
        let key = Self(Zeroizing::new(bytes));
        if key.0.iter().all(|byte| *byte == 0) {
            return Err(PersistenceError::InvalidKey);
        }
        Ok(key)
    }
}

impl CandidateAuthority {
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl SealedCandidate {
    pub const MAX_BYTES: usize = 16_384;

    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }
}

impl TorBoxClient {
    pub fn candidate_authority(
        &self,
        key: &CandidateKey,
    ) -> Result<CandidateAuthority, PersistenceError> {
        let log = workflow("candidate_authority", "torbox_candidate_authority_finished");
        finish(log, authority(self, key), "complete")
    }

    pub fn seal_candidate(
        &self,
        key: &CandidateKey,
        context: CandidateContext<'_>,
        candidate: &GeneratedFileCandidate,
    ) -> Result<SealedCandidate, PersistenceError> {
        let log = workflow("candidate_sealing", "torbox_candidate_sealing_finished");
        finish(log, seal(self, key, context, candidate), "sealed")
    }

    pub fn restore_candidate(
        &self,
        key: &CandidateKey,
        context: CandidateContext<'_>,
        sealed: &[u8],
    ) -> Result<GeneratedFileCandidate, PersistenceError> {
        let log = workflow(
            "candidate_restoration",
            "torbox_candidate_restoration_finished",
        );
        finish(log, restore(self, key, context, sealed), "restored")
    }
}

fn seal(
    client: &TorBoxClient,
    key: &CandidateKey,
    context: CandidateContext<'_>,
    candidate: &GeneratedFileCandidate,
) -> Result<SealedCandidate, PersistenceError> {
    if !candidate.belongs_to(&client.transport) || candidate.binding() != context.binding {
        return Err(PersistenceError::Rejected);
    }
    let authority = authority(client, key)?;
    let aad = associated_data(&context, &authority)?;
    let key = derive_key(key, b"encryption")?;
    let cipher = ChaCha20Poly1305::new_from_slice(key.as_ref())
        .map_err(|_| PersistenceError::Unavailable)?;
    let mut nonce = [0_u8; NONCE_BYTES];
    rand::rngs::OsRng
        .try_fill_bytes(&mut nonce)
        .map_err(|_| PersistenceError::Unavailable)?;
    let plain = Zeroizing::new(candidate.private_link().as_bytes().to_vec());
    let encrypted = cipher
        .encrypt(
            Nonce::from_slice(&nonce),
            Payload {
                msg: plain.as_ref(),
                aad: aad.as_ref(),
            },
        )
        .map_err(|_| PersistenceError::Unavailable)?;
    let mut sealed = Vec::with_capacity(1 + NONCE_BYTES + encrypted.len());
    sealed.push(VERSION);
    sealed.extend_from_slice(&nonce);
    sealed.extend_from_slice(&encrypted);
    if sealed.len() > SealedCandidate::MAX_BYTES {
        return Err(PersistenceError::Unavailable);
    }
    Ok(SealedCandidate(sealed))
}

fn restore(
    client: &TorBoxClient,
    key: &CandidateKey,
    context: CandidateContext<'_>,
    sealed: &[u8],
) -> Result<GeneratedFileCandidate, PersistenceError> {
    if !(1 + NONCE_BYTES + 16 + 1..=SealedCandidate::MAX_BYTES).contains(&sealed.len())
        || sealed[0] != VERSION
    {
        return Err(PersistenceError::Rejected);
    }
    let authority = authority(client, key)?;
    let aad = associated_data(&context, &authority)?;
    let key = derive_key(key, b"encryption")?;
    let cipher = ChaCha20Poly1305::new_from_slice(key.as_ref())
        .map_err(|_| PersistenceError::Unavailable)?;
    let plain = Zeroizing::new(
        cipher
            .decrypt(
                Nonce::from_slice(&sealed[1..1 + NONCE_BYTES]),
                Payload {
                    msg: &sealed[1 + NONCE_BYTES..],
                    aad: aad.as_ref(),
                },
            )
            .map_err(|_| PersistenceError::Rejected)?,
    );
    let link = Zeroizing::new(
        std::str::from_utf8(plain.as_ref())
            .map_err(|_| PersistenceError::Rejected)?
            .to_owned(),
    );
    GeneratedFileCandidate::new(context.binding, link, client.transport.candidate_origin())
        .map_err(|_| PersistenceError::Rejected)
}

fn authority(
    client: &TorBoxClient,
    key: &CandidateKey,
) -> Result<CandidateAuthority, PersistenceError> {
    let key = derive_key(key, b"credential-authority")?;
    client
        .transport
        .candidate_authority(&key)
        .map(CandidateAuthority)
}

fn derive_key(key: &CandidateKey, purpose: &[u8]) -> Result<Zeroizing<[u8; 32]>, PersistenceError> {
    let mut mac = <Hmac<Sha256> as Mac>::new_from_slice(key.0.as_ref())
        .map_err(|_| PersistenceError::Unavailable)?;
    mac.update(b"hito.torbox.candidate.keys.v1\0");
    mac.update(purpose);
    let mut digest = mac.finalize().into_bytes();
    let mut derived = Zeroizing::new([0_u8; 32]);
    derived.copy_from_slice(&digest);
    digest[..].zeroize();
    Ok(derived)
}

fn associated_data(
    context: &CandidateContext<'_>,
    authority: &CandidateAuthority,
) -> Result<Zeroizing<Vec<u8>>, PersistenceError> {
    if context.state_id.is_nil()
        || context.operation_id.is_nil()
        || context.handle.is_nil()
        || [context.recording_id, context.source_id]
            .iter()
            .any(|id| id.trim().is_empty() || id.len() > 256 || id.chars().any(char::is_control))
    {
        return Err(PersistenceError::Rejected);
    }
    context
        .binding
        .validate()
        .map_err(|_| PersistenceError::Rejected)?;
    let mut aad = Zeroizing::new(Vec::with_capacity(1024 + context.binding.file_name.len()));
    aad.extend_from_slice(DOMAIN);
    aad.push(VERSION);
    append_field(&mut aad, PROVIDER_ORIGIN.as_bytes());
    aad.extend_from_slice(context.state_id.as_bytes());
    aad.extend_from_slice(context.operation_id.as_bytes());
    aad.extend_from_slice(context.handle.as_bytes());
    append_field(&mut aad, context.recording_id.as_bytes());
    append_field(&mut aad, context.source_id.as_bytes());
    aad.extend_from_slice(&context.binding.torrent_id.to_be_bytes());
    append_field(&mut aad, context.binding.info_hash.as_bytes());
    aad.extend_from_slice(&context.binding.torrent_bytes.to_be_bytes());
    aad.extend_from_slice(&context.binding.file_id.to_be_bytes());
    append_field(&mut aad, context.binding.file_name.as_bytes());
    aad.extend_from_slice(&context.binding.file_bytes.to_be_bytes());
    aad.extend_from_slice(authority.as_bytes());
    Ok(aad)
}

fn append_field(bytes: &mut Vec<u8>, value: &[u8]) {
    bytes.extend_from_slice(&(value.len() as u32).to_be_bytes());
    bytes.extend_from_slice(value);
}

fn workflow(operation: &'static str, event: &'static str) -> WorkflowLog {
    WorkflowLog {
        started: Instant::now(),
        dispatch: tracing::dispatcher::get_default(Clone::clone),
        span: tracing::debug_span!("torbox_candidate_persistence", request_id = %Uuid::new_v4(), operation),
        event,
        outcome: "cancelled",
        count: 0,
        mint_dispatched: false,
    }
}

fn finish<T>(
    mut log: WorkflowLog,
    result: Result<T, PersistenceError>,
    success: &'static str,
) -> Result<T, PersistenceError> {
    log.outcome = match &result {
        Ok(_) => success,
        Err(PersistenceError::InvalidKey) => "invalid_key",
        Err(PersistenceError::Rejected) => "rejected",
        Err(PersistenceError::Unavailable) => "unavailable",
    };
    log.count = u64::from(result.is_ok());
    result
}
