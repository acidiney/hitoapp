use std::path::PathBuf;

use sha2::{Digest, Sha256};

use crate::Error;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    pub correction_recent_per_minute: u32,
    pub correction_pending_per_reporter: u32,
    pub correction_pending_total: u32,
    pub outcome_recent_per_minute: u32,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            correction_recent_per_minute: 10,
            correction_pending_per_reporter: 32,
            correction_pending_total: 1024,
            outcome_recent_per_minute: 300,
        }
    }
}

impl Limits {
    fn usable(&self) -> bool {
        [
            self.correction_recent_per_minute,
            self.correction_pending_per_reporter,
            self.correction_pending_total,
            self.outcome_recent_per_minute,
        ]
        .iter()
        .all(|limit| *limit > 0)
    }
}

pub struct Config {
    pub state_directory: PathBuf,
    pub admin_token: String,
    pub limits: Limits,
}

impl Config {
    pub(crate) fn validate(self) -> Result<(PathBuf, [u8; 32], Limits), Error> {
        if !self.limits.usable()
            || !self.state_directory.is_absolute()
            || self.state_directory.file_name().is_none()
            || self.admin_token.len() != 64
            || !self
                .admin_token
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(Error::InvalidConfiguration);
        }
        Ok((
            self.state_directory,
            Sha256::digest(self.admin_token.as_bytes()).into(),
            self.limits,
        ))
    }
}
