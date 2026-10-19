use crate::{DelayedFileReceipt, Error, MagnetFileBinding, magnet};
use serde::Deserialize;
use zeroize::Zeroize;

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
        (Some(delayed_id), None) if delayed_id > 0 => Ok(Acknowledgment::Delayed(
            DelayedFileReceipt::acknowledged(binding.clone(), delayed_id),
        )),
        (None, Some(link)) if !link.is_empty() && link.len() <= 8192 => {
            Ok(Acknowledgment::GeneratedUnsupported)
        }
        _ => Err(Error::InvalidResponse),
    }
}
