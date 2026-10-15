use super::root::Root;
use crate::{Error, Result};
use std::path::{Path, PathBuf};
use tokio::{
    sync::mpsc::{self, Receiver, Sender},
    task::JoinHandle,
};

const MAX_DEPTH: usize = 64;

pub(super) fn paths(root: &Root) -> (Receiver<PathBuf>, JoinHandle<Result<()>>) {
    let (sender, receiver) = mpsc::channel(32);
    let root = root.clone();
    let reader = tokio::task::spawn_blocking(move || walk(root, sender));
    (receiver, reader)
}

fn walk(root: Root, sender: Sender<PathBuf>) -> Result<()> {
    if sender.is_closed() {
        return Ok(());
    }
    let entries = root.directory.entries().map_err(|_| unreadable())?;
    let mut stack = vec![(PathBuf::new(), entries)];
    while !sender.is_closed() {
        let Some((parent, entries)) = stack.last_mut() else {
            break;
        };
        let Some(entry) = entries.next() else {
            stack.pop();
            continue;
        };
        if sender.is_closed() {
            break;
        }
        let entry = entry.map_err(|_| unreadable())?;
        let relative = parent.join(entry.file_name());
        let kind = entry.file_type().map_err(|_| unreadable())?;
        if kind.is_dir() {
            if stack.len() >= MAX_DEPTH {
                return Err(Error::Invalid(
                    "Music folder exceeds supported directory depth".into(),
                ));
            }
            let entries = root
                .directory
                .read_dir(&relative)
                .map_err(|_| unreadable())?;
            stack.push((relative, entries));
        } else if kind.is_file()
            && supported(&relative)
            && sender.blocking_send(root.path().join(relative)).is_err()
        {
            break;
        }
    }
    Ok(())
}

fn supported(path: &Path) -> bool {
    let extension = path
        .extension()
        .and_then(|v| v.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    [
        "mp3", "flac", "wav", "ogg", "opus", "m4a", "aac", "aiff", "aif", "mp4", "wma",
    ]
    .contains(&extension.as_str())
}

fn unreadable() -> Error {
    Error::Internal("Music folder could not be read".into())
}
