use crate::{Error, Result};
use std::{path::PathBuf, time::Duration};

#[derive(Clone)]
pub struct MediaTools {
    pub ffmpeg: PathBuf,
    pub ffprobe: PathBuf,
    pub timeout: Duration,
}

impl Default for MediaTools {
    fn default() -> Self {
        Self {
            ffmpeg: "ffmpeg".into(),
            ffprobe: "ffprobe".into(),
            timeout: Duration::from_secs(300),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostDeviceId(String);

impl HostDeviceId {
    pub fn new(value: impl Into<String>) -> Result<Self> {
        let value = value.into();
        if value.is_empty()
            || value.len() > 128
            || !value
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b"-_".contains(&c))
        {
            return Err(Error::Invalid(
                "Use a host device ID of 1 to 128 letters, numbers, hyphens, or underscores".into(),
            ));
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

pub struct CoreConfig {
    pub database: PathBuf,
    pub music_root: PathBuf,
    pub host_device: HostDeviceId,
    pub media_tools: MediaTools,
}

impl CoreConfig {
    pub fn new(database: PathBuf, music_root: PathBuf, host_device: HostDeviceId) -> Self {
        Self {
            database,
            music_root,
            host_device,
            media_tools: MediaTools::default(),
        }
    }
}
