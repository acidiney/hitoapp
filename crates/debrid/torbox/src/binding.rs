use crate::Error;
use serde::{Deserialize, Serialize};

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TorrentFileBinding {
    pub torrent_id: u64,
    pub info_hash: String,
    pub torrent_bytes: u64,
    pub file_id: u64,
    pub file_name: String,
    pub file_bytes: u64,
}

impl TorrentFileBinding {
    pub fn validate(&self) -> Result<(), Error> {
        if self.torrent_id == 0
            || self.info_hash.len() != 40
            || !self
                .info_hash
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
            || self.torrent_bytes == 0
            || self.file_bytes == 0
            || self.file_bytes > self.torrent_bytes
            || self.file_name.trim().is_empty()
            || self.file_name.len() > 4096
            || self.file_name.chars().any(char::is_control)
        {
            return Err(Error::InvalidBinding);
        }
        Ok(())
    }
}
