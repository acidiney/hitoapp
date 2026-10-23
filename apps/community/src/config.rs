use std::path::PathBuf;

use sha2::{Digest, Sha256};

use crate::Error;

pub struct Config {
    pub state_directory: PathBuf,
    pub admin_token: String,
}

impl Config {
    pub(crate) fn validate(self) -> Result<(PathBuf, [u8; 32]), Error> {
        if !self.state_directory.is_absolute()
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
        ))
    }
}
