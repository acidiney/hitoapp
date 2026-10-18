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
    pub storage: StorageConfig,
    pub host_device: HostDeviceId,
    pub media_tools: MediaTools,
    pub podcast_network: crate::PodcastNetworkConfig,
    pub community: Option<crate::CommunityConfig>,
}

impl CoreConfig {
    pub fn new(database: PathBuf, music_root: PathBuf, host_device: HostDeviceId) -> Self {
        Self {
            database,
            storage: StorageConfig::LegacyFolder(music_root),
            host_device,
            media_tools: MediaTools::default(),
            podcast_network: Default::default(),
            community: None,
        }
    }

    pub fn with_shared_directories(
        database: PathBuf,
        host_device: HostDeviceId,
        directories: Vec<SharedDirectoryConfig>,
    ) -> Self {
        Self {
            database,
            storage: StorageConfig::SharedDirectories(directories),
            host_device,
            media_tools: MediaTools::default(),
            podcast_network: Default::default(),
            community: None,
        }
    }
}

pub enum StorageConfig {
    LegacyFolder(PathBuf),
    SharedDirectories(Vec<SharedDirectoryConfig>),
}

#[derive(Clone, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SharedDirectoryConfig {
    pub id: String,
    pub name: String,
    pub root: PathBuf,
}
