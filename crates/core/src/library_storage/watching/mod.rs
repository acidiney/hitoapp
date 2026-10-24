use super::LibraryStorage;
use std::{collections::BTreeMap, time::Duration};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WatchMode {
    Off,
    Auto,
    Poll,
}

#[derive(Clone, Debug)]
pub struct FolderWatchSettings {
    pub mode: WatchMode,
    pub poll_interval: Duration,
    pub debounce: Duration,
}

impl Default for FolderWatchSettings {
    fn default() -> Self {
        Self {
            mode: WatchMode::Auto,
            poll_interval: Duration::from_secs(30),
            debounce: Duration::from_secs(2),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WatchBackend {
    Native,
    Polling,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WatchedFolder {
    pub folder_id: String,
    pub generation: i64,
    pub backend: WatchBackend,
}

pub type WatchStatus = BTreeMap<String, WatchedFolder>;

impl LibraryStorage {
    pub async fn watch_folders(
        &self,
        mut stopped: tokio::sync::watch::Receiver<bool>,
        _status: tokio::sync::watch::Sender<WatchStatus>,
    ) {
        let _ = stopped.changed().await;
    }
}
