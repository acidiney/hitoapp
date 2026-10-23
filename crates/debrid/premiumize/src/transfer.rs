use crate::Error;
use serde::{Deserialize, Serialize};

/// Trusted caller configuration, not proof of file, content or recording identity.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TransferBinding {
    pub transfer_id: String,
    pub created_at_unix_seconds: u64,
}

impl TransferBinding {
    pub fn validate(&self) -> Result<(), Error> {
        if self.transfer_id.trim().is_empty()
            || self.transfer_id.len() > 256
            || self.transfer_id.chars().any(char::is_control)
        {
            return Err(Error::InvalidBinding);
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransferPhase {
    Queued,
    Running,
    ProviderFailed,
    ProviderSeeding,
    ProviderCompleted,
}

#[derive(Clone, Copy, PartialEq)]
pub struct TransferProgress {
    fraction: f64,
}

impl TransferProgress {
    pub(crate) fn new(fraction: f64) -> Result<Self, Error> {
        if !fraction.is_finite() || !(0.0..=1.0).contains(&fraction) {
            return Err(Error::InvalidResponse);
        }
        Ok(Self { fraction })
    }

    pub fn fraction(self) -> f64 {
        self.fraction
    }
}

pub struct TransferObservation {
    pub binding: TransferBinding,
    pub phase: TransferPhase,
    pub progress: TransferProgress,
}
