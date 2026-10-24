use super::root::Root;
use crate::{Error, Result};
use std::path::{Path, PathBuf};
use tokio::{
    sync::{
        mpsc::{self, Receiver, Sender},
        watch,
    },
    time::{Duration, Instant},
};

const MAX_DEPTH: usize = 64;
const INACTIVITY: Duration = Duration::from_secs(5);

pub(super) struct Paths {
    receiver: Receiver<Event>,
    progress: watch::Receiver<Instant>,
}

enum Event {
    Path(PathBuf),
    Finished(Result<()>),
}

impl Paths {
    pub(super) async fn next(&mut self) -> Result<Option<PathBuf>> {
        let deadline = tokio::time::sleep_until(Instant::now() + INACTIVITY);
        tokio::pin!(deadline);
        loop {
            tokio::select! {
                biased;
                event = self.receiver.recv() => return match event {
                    Some(Event::Path(path)) => Ok(Some(path)),
                    Some(Event::Finished(result)) => { result?; Ok(None) }
                    None => Err(stopped()),
                },
                progress = self.progress.changed() => {
                    if progress.is_err() { return Err(stopped()); }
                    deadline.as_mut().reset(Instant::now() + INACTIVITY);
                },
                _ = &mut deadline => { self.receiver.close(); return Err(stopped()); }
            }
        }
    }
}

pub(super) fn paths(root: &Root) -> Result<Paths> {
    let permit = root
        .walk_slots
        .clone()
        .try_acquire_owned()
        .map_err(|_| stopped())?;
    let (sender, receiver) = mpsc::channel(32);
    let (activity, progress) = watch::channel(Instant::now());
    let root = root.clone();
    let span = tracing::Span::current();
    std::thread::Builder::new()
        .name("hito-folder-walk".into())
        .spawn(move || {
            let outcome = {
                let _permit = permit;
                span.in_scope(|| walk(root, &sender, &activity))
            };
            let _ = sender.blocking_send(Event::Finished(outcome));
        })
        .map_err(|_| stopped())?;
    Ok(Paths { receiver, progress })
}

fn walk(root: Root, sender: &Sender<Event>, progress: &watch::Sender<Instant>) -> Result<()> {
    if sender.is_closed() {
        return Ok(());
    }
    let entries = root.directory.entries().map_err(|_| unreadable())?;
    progress.send_replace(Instant::now());
    let mut stack = vec![(PathBuf::new(), entries)];
    while !sender.is_closed() {
        let Some((parent, entries)) = stack.last_mut() else {
            break;
        };
        let entry = entries.next();
        progress.send_replace(Instant::now());
        if sender.is_closed() {
            break;
        }
        let Some(entry) = entry else {
            stack.pop();
            continue;
        };
        if sender.is_closed() {
            break;
        }
        let entry = entry.map_err(|_| unreadable())?;
        let relative = parent.join(entry.file_name());
        let kind = entry.file_type().map_err(|_| unreadable())?;
        progress.send_replace(Instant::now());
        if sender.is_closed() {
            break;
        }
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
            progress.send_replace(Instant::now());
            stack.push((relative, entries));
        } else if kind.is_file()
            && supported(&relative)
            && sender
                .blocking_send(Event::Path(root.path().join(relative)))
                .is_err()
        {
            break;
        }
    }
    Ok(())
}

pub(super) fn supported(path: &Path) -> bool {
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

fn stopped() -> Error {
    Error::Internal("Music folder stopped making progress. Try scanning again".into())
}
