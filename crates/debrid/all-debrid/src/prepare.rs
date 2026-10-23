use crate::{DelayedFileReceipt, Error, GeneratedFileCandidate, MagnetFileBinding, magnet};
use serde::Deserialize;
use zeroize::Zeroize;

pub enum PreparedFile {
    Delayed(DelayedFileReceipt),
    Candidate(GeneratedFileCandidate),
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

pub(crate) fn acknowledgment(
    bytes: &[u8],
    binding: &MagnetFileBinding,
) -> Result<PreparedFile, Error> {
    let mut data: UnlockData = magnet::parse(bytes)?;
    if data.filesize != binding.file_bytes {
        return Err(Error::InvalidResponse);
    }
    match (data.delayed, data.link.as_deref()) {
        (Some(delayed_id), None) if delayed_id > 0 => Ok(PreparedFile::Delayed(
            DelayedFileReceipt::acknowledged(binding.clone(), delayed_id),
        )),
        (None, Some(_)) => Ok(PreparedFile::Candidate(
            GeneratedFileCandidate::acknowledged(
                binding.clone(),
                data.link.take().ok_or(Error::InvalidResponse)?,
            )?,
        )),
        _ => Err(Error::InvalidResponse),
    }
}
