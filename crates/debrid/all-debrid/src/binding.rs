use crate::Error;
use serde::{Deserialize, Serialize};

pub(crate) const MAX_DEPTH: usize = 16;
pub(crate) const MAX_PATH_BYTES: usize = 4096;

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MagnetFileBinding {
    pub magnet_id: u64,
    pub info_hash: String,
    pub magnet_bytes: u64,
    pub file_path: Vec<String>,
    pub file_bytes: u64,
}

impl MagnetFileBinding {
    pub fn validate(&self) -> Result<(), Error> {
        if self.magnet_id == 0 {
            return Err(Error::InvalidBinding);
        }
        validate_selection(
            &self.info_hash,
            self.magnet_bytes,
            &self.file_path,
            self.file_bytes,
        )
    }
}

pub(crate) fn validate_selection(
    info_hash: &str,
    magnet_bytes: u64,
    file_path: &[String],
    file_bytes: u64,
) -> Result<(), Error> {
    if info_hash.len() != 40
        || !info_hash
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        || magnet_bytes == 0
        || file_bytes == 0
        || file_bytes > magnet_bytes
        || file_path.is_empty()
        || file_path.len() > MAX_DEPTH
        || file_path.iter().any(|segment| !canonical_segment(segment))
        || file_path
            .iter()
            .map(|segment| segment.len() + 1)
            .sum::<usize>()
            - 1
            > MAX_PATH_BYTES
    {
        return Err(Error::InvalidBinding);
    }
    Ok(())
}

pub(crate) fn canonical_segment(value: &str) -> bool {
    !value.trim().is_empty()
        && value.len() <= 255
        && !matches!(value, "." | "..")
        && !value
            .chars()
            .any(|value| value.is_control() || matches!(value, '/' | '\\'))
}
