use crate::{Error, MagnetFileBinding, magnet};
use serde::{Deserialize, Deserializer, Serialize};
use zeroize::Zeroize;

/// Delayed unlock metadata. Stored snapshots require trusted ownership.
#[derive(Serialize)]
pub struct DelayedFileReceipt {
    binding: MagnetFileBinding,
    delayed_id: u64,
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
        Ok(Self {
            binding: stored.binding,
            delayed_id: stored.delayed_id,
        })
    }
}

impl DelayedFileReceipt {
    pub fn binding(&self) -> &MagnetFileBinding {
        &self.binding
    }

    pub fn poll_after_seconds(&self) -> u32 {
        5
    }
}

#[derive(Deserialize)]
struct UnlockData {
    filesize: u64,
    delayed: Option<u64>,
    link: Option<String>,
}

impl Drop for UnlockData {
    fn drop(&mut self) {
        if let Some(link) = &mut self.link {
            link.zeroize();
        }
    }
}

pub(crate) enum Acknowledgment {
    Delayed(DelayedFileReceipt),
    GeneratedUnsupported,
}

pub(crate) fn acknowledgment(
    bytes: &[u8],
    binding: &MagnetFileBinding,
) -> Result<Acknowledgment, Error> {
    let data: UnlockData = magnet::parse(bytes)?;
    if data.filesize != binding.file_bytes {
        return Err(Error::InvalidResponse);
    }
    match (data.delayed, data.link.as_deref()) {
        (Some(delayed_id), None) if delayed_id > 0 => {
            Ok(Acknowledgment::Delayed(DelayedFileReceipt {
                binding: binding.clone(),
                delayed_id,
            }))
        }
        (None, Some(link)) if !link.is_empty() && link.len() <= 8192 => {
            Ok(Acknowledgment::GeneratedUnsupported)
        }
        _ => Err(Error::InvalidResponse),
    }
}
