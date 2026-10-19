use crate::{Error, MagnetFileBinding, magnet};
use serde::{Deserialize, Deserializer, Serialize};
use std::time::Duration;
use tokio::time::Instant;
use zeroize::Zeroize;

const POLL_INTERVAL: Duration = Duration::from_secs(5);
const MAX_TIME_LEFT: u32 = 7 * 24 * 60 * 60;

/// Delayed unlock metadata. Stored snapshots require trusted ownership.
#[derive(Serialize)]
pub struct DelayedFileReceipt {
    binding: MagnetFileBinding,
    delayed_id: u64,
    #[serde(skip)]
    next_poll_at: Instant,
}

impl<'de> Deserialize<'de> for DelayedFileReceipt {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Stored {
            binding: MagnetFileBinding,
            delayed_id: u64,
        }
        let stored = Stored::deserialize(deserializer)?;
        stored
            .binding
            .validate()
            .map_err(|_| serde::de::Error::custom("The stored AllDebrid binding is invalid"))?;
        if stored.delayed_id == 0 {
            return Err(serde::de::Error::custom(
                "The stored AllDebrid delayed ID is invalid",
            ));
        }
        Ok(Self::acknowledged(stored.binding, stored.delayed_id))
    }
}

impl DelayedFileReceipt {
    pub(crate) fn acknowledged(binding: MagnetFileBinding, delayed_id: u64) -> Self {
        Self {
            binding,
            delayed_id,
            next_poll_at: Instant::now() + POLL_INTERVAL,
        }
    }

    pub fn binding(&self) -> &MagnetFileBinding {
        &self.binding
    }

    pub fn poll_after_seconds(&self) -> u32 {
        POLL_INTERVAL.as_secs() as u32
    }

    pub(crate) fn attempt(&mut self) -> PollAttempt<'_> {
        PollAttempt {
            binding: &self.binding,
            delayed_id: self.delayed_id,
            next_poll_at: &mut self.next_poll_at,
        }
    }
}

pub(crate) struct PollAttempt<'a> {
    pub binding: &'a MagnetFileBinding,
    pub delayed_id: u64,
    next_poll_at: &'a mut Instant,
}

impl PollAttempt<'_> {
    pub async fn wait(&self) {
        tokio::time::sleep_until(*self.next_poll_at).await;
    }
}

impl Drop for PollAttempt<'_> {
    fn drop(&mut self) {
        *self.next_poll_at = Instant::now() + POLL_INTERVAL;
    }
}

pub enum DelayedPoll {
    Processing {
        poll_after_seconds: u32,
        time_left_seconds: u32,
    },
    Candidate(GeneratedFileCandidate),
}

pub struct GeneratedFileCandidate {
    binding: MagnetFileBinding,
    link: String,
}

impl Drop for GeneratedFileCandidate {
    fn drop(&mut self) {
        self.link.zeroize();
    }
}

#[derive(Deserialize)]
struct PollData {
    status: u8,
    time_left: u32,
    link: Option<String>,
}

impl Drop for PollData {
    fn drop(&mut self) {
        if let Some(link) = &mut self.link {
            link.zeroize();
        }
    }
}

pub(crate) fn observation(bytes: &[u8], binding: &MagnetFileBinding) -> Result<DelayedPoll, Error> {
    let mut data: PollData = magnet::parse(bytes)?;
    if data.time_left > MAX_TIME_LEFT {
        return Err(Error::InvalidResponse);
    }
    match data.status {
        1 if data.link.is_none() => Ok(DelayedPoll::Processing {
            poll_after_seconds: POLL_INTERVAL.as_secs() as u32,
            time_left_seconds: data.time_left,
        }),
        2 => {
            let link = data.link.as_deref().ok_or(Error::InvalidResponse)?;
            magnet::check_link(link)?;
            Ok(DelayedPoll::Candidate(GeneratedFileCandidate {
                binding: binding.clone(),
                link: data.link.take().ok_or(Error::InvalidResponse)?,
            }))
        }
        3 if data.link.is_none() => Err(Error::ProviderFailed),
        _ => Err(Error::InvalidResponse),
    }
}

impl GeneratedFileCandidate {
    pub fn binding(&self) -> &MagnetFileBinding {
        &self.binding
    }

    pub(crate) fn private_link(&self) -> &str {
        &self.link
    }
}
