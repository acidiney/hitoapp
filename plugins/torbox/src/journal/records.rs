use crate::{CuratedWavRecording, StateError};
use hito_debrid_torbox::{
    CandidateContext, Error as ProviderError, SealedCandidate, TorrentFileBinding,
};
use hito_plugin_sdk::{PreparationReason, PreparationStatus};
use hmac::{Hmac, Mac};
use sha2::Sha256;
use sqlx::{Row, SqliteConnection, sqlite::SqliteRow};
use std::time::{SystemTime, UNIX_EPOCH};
use subtle::ConstantTimeEq;
use uuid::Uuid;

pub(super) const MAX_RECEIPTS: usize = 1024;
const MAX_BINDING_BYTES: usize = 16_384;

pub(super) enum WriteError {
    State(StateError),
    Database(sqlx::Error),
}

impl WriteError {
    pub(super) fn is_full(&self) -> bool {
        match self {
            Self::Database(error) => {
                error
                    .as_database_error()
                    .and_then(|error| error.code())
                    .as_deref()
                    == Some("13")
            }
            Self::State(_) => false,
        }
    }
}

impl From<StateError> for WriteError {
    fn from(error: StateError) -> Self {
        Self::State(error)
    }
}

impl From<WriteError> for StateError {
    fn from(error: WriteError) -> Self {
        match error {
            WriteError::State(error) => error,
            WriteError::Database(error) => super::database_error(error),
        }
    }
}

#[derive(Clone, PartialEq, Eq)]
pub(crate) struct Target {
    pub(super) recording: String,
    pub(super) source: String,
    pub(super) binding: TorrentFileBinding,
}

impl From<&CuratedWavRecording> for Target {
    fn from(entry: &CuratedWavRecording) -> Self {
        Self {
            recording: entry.recording.id.clone(),
            source: entry.source_id.clone(),
            binding: entry.binding.clone(),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(i64)]
pub(super) enum Phase {
    Checking = 0,
    MintIntent = 1,
    Preparing = 2,
    Blocked = 3,
    Ready = 4,
    Unknown = 5,
    Failed = 6,
}

#[derive(Clone, PartialEq, Eq)]
pub(super) struct Record {
    pub(super) operation: Uuid,
    pub(super) handle: Uuid,
    pub(super) target: Target,
    pub(super) authority: [u8; 32],
    pub(super) phase: Phase,
    pub(super) reason: Option<PreparationReason>,
    pub(super) retry_at_ms: i64,
    pub(super) revision: i64,
    pub(super) current: bool,
    pub(super) candidate: Vec<u8>,
    pub(super) candidate_length: usize,
}

impl Record {
    pub(super) fn new(operation: Uuid, target: Target, authority: [u8; 32]) -> Self {
        Self {
            operation,
            handle: Uuid::new_v4(),
            target,
            authority,
            phase: Phase::Checking,
            reason: None,
            retry_at_ms: 0,
            revision: 0,
            current: true,
            candidate: vec![0; SealedCandidate::MAX_BYTES],
            candidate_length: 0,
        }
    }

    pub(super) fn context(&self, state: Uuid) -> CandidateContext<'_> {
        CandidateContext {
            state_id: state,
            operation_id: self.operation,
            handle: self.handle,
            recording_id: &self.target.recording,
            source_id: &self.target.source,
            binding: &self.target.binding,
        }
    }

    pub(super) fn status(&self) -> PreparationStatus {
        match self.phase {
            Phase::Checking | Phase::MintIntent | Phase::Unknown => {
                PreparationStatus::SubmissionUnknown {}
            }
            Phase::Preparing => PreparationStatus::Preparing {
                progress_basis_points: None,
                eta_seconds: None,
                poll_after_seconds: 5,
            },
            Phase::Blocked => PreparationStatus::Blocked {
                reason: self
                    .reason
                    .unwrap_or(PreparationReason::ProviderUnavailable),
                retry_after_seconds: Some(5),
            },
            Phase::Ready => PreparationStatus::Ready {},
            Phase::Failed => PreparationStatus::Failed {
                reason: self.reason.unwrap_or(PreparationReason::SourceUnavailable),
            },
        }
    }

    pub(super) fn next(
        &self,
        phase: Phase,
        reason: Option<PreparationReason>,
    ) -> Result<Self, StateError> {
        let mut next = self.clone();
        next.revision = self
            .revision
            .checked_add(1)
            .ok_or(StateError::InvalidState)?;
        next.phase = phase;
        next.reason = reason;
        next.retry_at_ms = if matches!(phase, Phase::Preparing | Phase::Blocked) {
            now_ms()?.checked_add(5000).ok_or(StateError::Unavailable)?
        } else {
            0
        };
        next.candidate_length = 0;
        if phase == Phase::Failed {
            next.candidate.clear();
        } else {
            next.candidate.fill(0);
        }
        Ok(next)
    }

    pub(super) fn validate(&self) -> Result<(), StateError> {
        let invalid_id =
            |id: &str| id.trim().is_empty() || id.len() > 256 || id.chars().any(char::is_control);
        if self.operation.is_nil()
            || self.handle.get_version() != Some(uuid::Version::Random)
            || invalid_id(&self.target.recording)
            || invalid_id(&self.target.source)
            || self.target.binding.validate().is_err()
            || self.revision < 0
            || self.retry_at_ms < 0
            || matches!(self.phase, Phase::Preparing | Phase::Blocked) != (self.retry_at_ms > 0)
            || matches!(self.phase, Phase::Blocked | Phase::Failed) != self.reason.is_some()
            || (!self.current && self.phase != Phase::Failed)
        {
            return Err(StateError::InvalidState);
        }
        binding_bytes(&self.target.binding)?;
        let expected = if self.phase == Phase::Failed {
            0
        } else {
            SealedCandidate::MAX_BYTES
        };
        if self.candidate.len() != expected
            || self.candidate_length > self.candidate.len()
            || (self.phase == Phase::Ready && self.candidate_length < 30)
            || (self.phase != Phase::Ready && self.candidate_length != 0)
            || self.candidate[self.candidate_length..]
                .iter()
                .any(|byte| *byte != 0)
        {
            return Err(StateError::InvalidState);
        }
        Ok(())
    }

    fn mac(&self, key: &[u8; 32], state: Uuid) -> Result<Hmac<Sha256>, StateError> {
        let mut mac =
            <Hmac<Sha256> as Mac>::new_from_slice(key).map_err(|_| StateError::Unavailable)?;
        mac.update(b"hito.torbox.journal.row.v4\0");
        mac.update(state.as_bytes());
        mac.update(self.operation.as_bytes());
        mac.update(self.handle.as_bytes());
        let binding = binding_bytes(&self.target.binding)?;
        for field in [
            self.target.recording.as_bytes(),
            self.target.source.as_bytes(),
            binding.as_slice(),
            self.authority.as_slice(),
        ] {
            mac.update(&(field.len() as u64).to_be_bytes());
            mac.update(field);
        }
        for field in [
            self.phase as i64,
            reason_code(self.reason),
            self.retry_at_ms,
            self.revision,
            self.current as i64,
            self.candidate_length as i64,
        ] {
            mac.update(&field.to_be_bytes());
        }
        mac.update(&(self.candidate.len() as u64).to_be_bytes());
        mac.update(&self.candidate);
        Ok(mac)
    }

    fn decode(
        row: SqliteRow,
        key: &[u8; 32],
        state: Uuid,
        authority: &[u8; 32],
    ) -> Result<Self, StateError> {
        fn invalid<T>(_: T) -> StateError {
            StateError::InvalidState
        }
        let operation =
            Uuid::from_slice(&row.try_get::<Vec<u8>, _>("operation_id").map_err(invalid)?)
                .map_err(invalid)?;
        let handle = Uuid::from_slice(&row.try_get::<Vec<u8>, _>("handle").map_err(invalid)?)
            .map_err(invalid)?;
        let binding: Vec<u8> = row.try_get("binding").map_err(invalid)?;
        if binding.len() > MAX_BINDING_BYTES {
            return Err(StateError::InvalidState);
        }
        let decoded: TorrentFileBinding = serde_json::from_slice(&binding).map_err(invalid)?;
        if binding_bytes(&decoded)? != binding {
            return Err(StateError::InvalidState);
        }
        let binding = decoded;
        let saved: [u8; 32] = row
            .try_get::<Vec<u8>, _>("authority")
            .map_err(invalid)?
            .try_into()
            .map_err(invalid)?;
        if !bool::from(saved.ct_eq(authority)) {
            return Err(StateError::AuthorityChanged);
        }
        let phase = match decode_scalar(&row, "phase")? {
            0 => Phase::Checking,
            1 => Phase::MintIntent,
            2 => Phase::Preparing,
            3 => Phase::Blocked,
            4 => Phase::Ready,
            5 => Phase::Unknown,
            6 => Phase::Failed,
            _ => return Err(StateError::InvalidState),
        };
        let current = match decode_scalar(&row, "current")? {
            0 => false,
            1 => true,
            _ => return Err(StateError::InvalidState),
        };
        let record = Self {
            operation,
            handle,
            target: Target {
                recording: row.try_get("recording_id").map_err(invalid)?,
                source: row.try_get("source_id").map_err(invalid)?,
                binding,
            },
            authority: saved,
            phase,
            reason: decode_reason(decode_scalar(&row, "reason")?)?,
            retry_at_ms: decode_scalar(&row, "retry_at_ms")?,
            revision: decode_scalar(&row, "revision")?,
            current,
            candidate: row.try_get("candidate").map_err(invalid)?,
            candidate_length: usize::try_from(decode_scalar(&row, "candidate_length")?)
                .map_err(invalid)?,
        };
        record.validate()?;
        let saved_mac: Vec<u8> = row.try_get("mac").map_err(invalid)?;
        record
            .mac(key, state)?
            .verify_slice(&saved_mac)
            .map_err(invalid)?;
        Ok(record)
    }
}

pub(super) async fn load(
    connection: &mut SqliteConnection,
    key: &[u8; 32],
    state: Uuid,
    authority: &[u8; 32],
    operation: Uuid,
) -> Result<Option<Record>, StateError> {
    sqlx::query("SELECT * FROM operations WHERE operation_id = ?")
        .bind(operation.as_bytes().as_slice())
        .fetch_optional(connection)
        .await
        .map_err(super::database_error)?
        .map(|row| Record::decode(row, key, state, authority))
        .transpose()
}

pub(super) async fn current(
    connection: &mut SqliteConnection,
    key: &[u8; 32],
    state: Uuid,
    authority: &[u8; 32],
    target: &Target,
) -> Result<Option<Record>, StateError> {
    sqlx::query("SELECT * FROM operations WHERE recording_id = ? AND source_id = ? AND current = X'0000000000000001'")
        .bind(&target.recording).bind(&target.source).fetch_optional(connection).await.map_err(super::database_error)?
        .map(|row| Record::decode(row, key, state, authority)).transpose()
}

pub(super) async fn claims(
    connection: &mut SqliteConnection,
    key: &[u8; 32],
    state: Uuid,
    authority: &[u8; 32],
    target: &Target,
) -> Result<Vec<Record>, StateError> {
    let rows = sqlx::query("SELECT * FROM operations WHERE current = X'0000000000000001' AND (recording_id = ? OR source_id = ?) LIMIT 1025")
        .bind(&target.recording).bind(&target.source).fetch_all(connection).await.map_err(super::database_error)?;
    if rows.len() > MAX_RECEIPTS {
        return Err(StateError::InvalidState);
    }
    rows.into_iter()
        .map(|row| Record::decode(row, key, state, authority))
        .collect()
}

pub(super) async fn all(
    connection: &mut SqliteConnection,
    key: &[u8; 32],
    state: Uuid,
    authority: &[u8; 32],
) -> Result<Vec<Record>, StateError> {
    let rows = sqlx::query("SELECT * FROM operations LIMIT 1025")
        .fetch_all(connection)
        .await
        .map_err(super::database_error)?;
    if rows.len() > MAX_RECEIPTS {
        return Err(StateError::InvalidState);
    }
    rows.into_iter()
        .map(|row| Record::decode(row, key, state, authority))
        .collect()
}

pub(super) async fn insert(
    connection: &mut SqliteConnection,
    record: &Record,
    key: &[u8; 32],
    state: Uuid,
) -> Result<(), WriteError> {
    record.validate()?;
    let mac = record.mac(key, state)?.finalize().into_bytes();
    sqlx::query("INSERT INTO operations VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)")
        .bind(record.operation.as_bytes().as_slice())
        .bind(record.handle.as_bytes().as_slice())
        .bind(&record.target.recording)
        .bind(&record.target.source)
        .bind(binding_bytes(&record.target.binding)?)
        .bind(record.authority.as_slice())
        .bind(encode_scalar(record.phase as i64))
        .bind(encode_scalar(reason_code(record.reason)))
        .bind(encode_scalar(record.retry_at_ms))
        .bind(encode_scalar(record.revision))
        .bind(encode_scalar(record.current as i64))
        .bind(&record.candidate)
        .bind(encode_scalar(record.candidate_length as i64))
        .bind(mac.as_slice())
        .execute(connection)
        .await
        .map_err(WriteError::Database)?;
    Ok(())
}

pub(super) async fn update(
    connection: &mut SqliteConnection,
    old: &Record,
    next: &Record,
    key: &[u8; 32],
    state: Uuid,
) -> Result<(), WriteError> {
    next.validate()?;
    if old.operation != next.operation
        || old.handle != next.handle
        || old.target != next.target
        || old.authority != next.authority
        || old.revision.checked_add(1) != Some(next.revision)
    {
        return Err(StateError::InvalidState.into());
    }
    let mac = next.mac(key, state)?.finalize().into_bytes();
    let changed = sqlx::query("UPDATE operations SET phase = ?, reason = ?, retry_at_ms = ?, revision = ?, current = ?, candidate = ?, candidate_length = ?, mac = ? WHERE operation_id = ? AND handle = ? AND recording_id = ? AND source_id = ? AND binding = ? AND authority = ? AND revision = ? AND current = ?")
        .bind(encode_scalar(next.phase as i64)).bind(encode_scalar(reason_code(next.reason))).bind(encode_scalar(next.retry_at_ms)).bind(encode_scalar(next.revision)).bind(encode_scalar(next.current as i64))
        .bind(&next.candidate).bind(encode_scalar(next.candidate_length as i64)).bind(mac.as_slice())
        .bind(old.operation.as_bytes().as_slice()).bind(old.handle.as_bytes().as_slice()).bind(&old.target.recording).bind(&old.target.source)
        .bind(binding_bytes(&old.target.binding)?).bind(old.authority.as_slice()).bind(encode_scalar(old.revision)).bind(encode_scalar(old.current as i64))
        .execute(connection).await.map_err(WriteError::Database)?.rows_affected();
    if changed != 1 {
        return Err(StateError::InvalidState.into());
    }
    Ok(())
}

pub(super) fn failed_phase(error: ProviderError) -> (Phase, Option<PreparationReason>) {
    use PreparationReason as Reason;
    match error {
        ProviderError::SourceIncomplete => (Phase::Preparing, None),
        ProviderError::LinkUnknown => (Phase::Unknown, None),
        ProviderError::Status(401 | 403) | ProviderError::InvalidCredential => {
            (Phase::Failed, Some(Reason::AuthorizationRequired))
        }
        ProviderError::SourceChanged => (Phase::Failed, Some(Reason::SourceChanged)),
        ProviderError::ProviderRejected => (Phase::Failed, Some(Reason::SourceUnavailable)),
        ProviderError::ResponseTooLarge | ProviderError::InvalidResponse => {
            (Phase::Failed, Some(Reason::InvalidResponse))
        }
        ProviderError::Busy | ProviderError::Status(429) => {
            (Phase::Blocked, Some(Reason::RateLimited))
        }
        _ => (Phase::Blocked, Some(Reason::ProviderUnavailable)),
    }
}

pub(super) fn now_ms() -> Result<i64, StateError> {
    let elapsed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| StateError::Unavailable)?;
    i64::try_from(elapsed.as_millis()).map_err(|_| StateError::Unavailable)
}

pub(super) fn binding_bytes(binding: &TorrentFileBinding) -> Result<Vec<u8>, StateError> {
    let bytes = serde_json::to_vec(binding).map_err(|_| StateError::InvalidState)?;
    if bytes.len() > MAX_BINDING_BYTES {
        return Err(StateError::InvalidState);
    }
    Ok(bytes)
}

fn encode_scalar(value: i64) -> Vec<u8> {
    value.to_be_bytes().to_vec()
}

fn decode_scalar(row: &SqliteRow, column: &str) -> Result<i64, StateError> {
    let bytes: [u8; 8] = row
        .try_get::<Vec<u8>, _>(column)
        .map_err(|_| StateError::InvalidState)?
        .try_into()
        .map_err(|_| StateError::InvalidState)?;
    Ok(i64::from_be_bytes(bytes))
}

fn reason_code(reason: Option<PreparationReason>) -> i64 {
    match reason {
        None => 0,
        Some(PreparationReason::AuthorizationRequired) => 1,
        Some(PreparationReason::QuotaReached) => 2,
        Some(PreparationReason::RateLimited) => 3,
        Some(PreparationReason::ProviderRestricted) => 4,
        Some(PreparationReason::SourceUnavailable) => 5,
        Some(PreparationReason::SourceChanged) => 6,
        Some(PreparationReason::Unsupported) => 7,
        Some(PreparationReason::ProviderUnavailable) => 8,
        Some(PreparationReason::InvalidResponse) => 9,
    }
}

fn decode_reason(reason: i64) -> Result<Option<PreparationReason>, StateError> {
    Ok(match reason {
        0 => None,
        1 => Some(PreparationReason::AuthorizationRequired),
        2 => Some(PreparationReason::QuotaReached),
        3 => Some(PreparationReason::RateLimited),
        4 => Some(PreparationReason::ProviderRestricted),
        5 => Some(PreparationReason::SourceUnavailable),
        6 => Some(PreparationReason::SourceChanged),
        7 => Some(PreparationReason::Unsupported),
        8 => Some(PreparationReason::ProviderUnavailable),
        9 => Some(PreparationReason::InvalidResponse),
        _ => return Err(StateError::InvalidState),
    })
}
