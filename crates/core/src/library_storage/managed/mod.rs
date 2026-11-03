mod lifecycle;
#[cfg(unix)]
mod private_root;
mod publication;
mod reading;

use super::{FileInspector, Root, blocking};
use crate::{Error, Result};
use cap_fs_ext::{FollowSymlinks, OpenOptionsFollowExt};
use cap_std::fs::OpenOptions;
pub(crate) use lifecycle::ManagedAttempt;
use lifecycle::{Writer, Writers};
use publication::sync_directory;
use std::{
    fs::File,
    io::Write,
    path::Path,
    sync::{Arc, Mutex},
};
#[cfg(unix)]
use tokio::sync::Semaphore;
use uuid::Uuid;

pub(crate) const MAX_MANAGED_BYTES: u64 = 2 * 1024 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ManagedFileLocation {
    pub file_id: Uuid,
    pub attempt_id: Uuid,
    pub generation: i64,
    pub checksum: String,
    pub byte_length: u64,
    pub mime: &'static str,
}

#[derive(Clone)]
pub(crate) struct ManagedFiles {
    root: Option<Root>,
    inspector: FileInspector,
    writers: Writers,
}

pub(crate) struct PendingManagedFile {
    file_id: Uuid,
    attempt_id: Uuid,
    file: Arc<Mutex<File>>,
    length: u64,
    root: Root,
    writer: Writer,
}

pub(crate) struct VerifiedManagedFile {
    location: ManagedFileLocation,
    root: Root,
    writer: Writer,
}

impl ManagedFiles {
    pub(crate) fn active(&self, file_id: Uuid, attempt_id: Uuid) -> Result<bool> {
        self.writers.active(file_id, attempt_id)
    }

    pub(crate) fn reserve(&self, file_id: Uuid, attempt_id: Uuid) -> Result<ManagedAttempt> {
        self.writers.reserve(file_id, attempt_id)
    }

    pub(crate) fn revoke(&self, file_id: Uuid, attempt_id: Uuid) -> Result<()> {
        self.writers.revoke(file_id, attempt_id).map(|_| ())
    }

    pub(crate) fn ensure_supported(&self) -> Result<()> {
        self.root().map(|_| ())
    }

    #[tracing::instrument(level = "debug", skip_all, err(Debug))]
    pub(crate) async fn open(database: &Path, inspector: FileInspector) -> Result<Self> {
        #[cfg(unix)]
        let root = {
            let database = database.to_owned();
            let slots = Arc::new(Semaphore::new(1));
            let owned_slots = slots.clone();
            Some(
                blocking::run(slots, move |_| {
                    let (path, directory) = private_root::open(&database)?;
                    Ok(Root::from_owned_dir(path, directory, owned_slots))
                })
                .await?,
            )
        };
        #[cfg(not(unix))]
        let root = {
            let _ = database;
            None
        };
        tracing::debug!(event = "download_storage_opened", enabled = root.is_some());
        let files = Self {
            root,
            inspector,
            writers: Writers::default(),
        };
        if let Err(error) = files.sweep_staging().await {
            tracing::warn!(
                event = "download_staging_sweep_failed",
                error_code = error.code(),
                "Leftover download staging files could not be removed"
            );
        }
        Ok(files)
    }

    #[tracing::instrument(level = "debug", skip_all, err(Debug), fields(file_id = %attempt.file_id, attempt_id = %attempt.attempt_id))]
    pub(crate) async fn begin(&self, attempt: &ManagedAttempt) -> Result<PendingManagedFile> {
        let root = self.root()?.clone();
        let file_id = attempt.file_id;
        let attempt_id = attempt.attempt_id;
        let writer = attempt.begin().await?;
        let owned_root = root.clone();
        let owned_writer = writer.clone();
        let file = blocking::run(root.slots.clone(), move |_| {
            let writer = owned_writer;
            writer.check()?;
            let mut options = OpenOptions::new();
            options
                .write(true)
                .create_new(true)
                .follow(FollowSymlinks::No);
            #[cfg(unix)]
            {
                use cap_std::fs::OpenOptionsExt;
                options.mode(0o600).custom_flags(libc::O_NONBLOCK);
            }
            let file = owned_root
                .directory
                .open_with(name(file_id, attempt_id, "part"), &options)
                .map_err(|error| storage_failure(&error))?
                .into_std();
            private_file(&file, None)?;
            Ok(file)
        })
        .await?;
        Ok(PendingManagedFile {
            file_id,
            attempt_id,
            file: Arc::new(Mutex::new(file)),
            length: 0,
            root,
            writer,
        })
    }

    #[tracing::instrument(level = "debug", skip_all, err(Debug), fields(%file_id, %attempt_id))]
    pub(crate) async fn remove_owned(&self, file_id: Uuid, attempt_id: Uuid) -> Result<()> {
        let root = self.root()?.clone();
        let writer = self.writers.revoke(file_id, attempt_id)?.cleanup().await?;
        blocking::run(root.slots.clone(), move |_| {
            let _writer = writer;
            for extension in ["part", "media"] {
                match root
                    .directory
                    .remove_file(name(file_id, attempt_id, extension))
                {
                    Ok(()) => {}
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                    Err(_) => return Err(unavailable()),
                }
            }
            sync_directory(&root)?;
            tracing::debug!(event = "download_file_removed");
            Ok(())
        })
        .await
    }

    fn root(&self) -> Result<&Root> {
        self.root.as_ref().ok_or_else(|| {
            Error::Invalid("Private downloads are not supported on this platform yet".into())
        })
    }
}

impl VerifiedManagedFile {
    pub(crate) fn location(&self) -> &ManagedFileLocation {
        &self.location
    }
}

impl PendingManagedFile {
    pub(crate) async fn write_chunk(&mut self, bytes: &[u8]) -> Result<()> {
        let length = self
            .length
            .checked_add(bytes.len() as u64)
            .filter(|length| *length <= MAX_MANAGED_BYTES)
            .ok_or_else(|| Error::Invalid("Downloaded audio exceeds the supported size".into()))?;
        let bytes = bytes.to_owned();
        let file = self.file.clone();
        let writer = self.writer.clone();
        blocking::run(self.root.slots.clone(), move |cancelled| {
            let writer = writer;
            writer.check()?;
            blocking::check(cancelled)?;
            file.lock()
                .map_err(|_| unavailable())?
                .write_all(&bytes)
                .map_err(|error| storage_failure(&error))
        })
        .await?;
        self.length = length;
        Ok(())
    }
}

pub(crate) fn managed_mime(value: &str) -> Option<&'static str> {
    match value {
        "audio/wav" => Some("audio/wav"),
        "audio/flac" => Some("audio/flac"),
        "audio/mpeg" => Some("audio/mpeg"),
        "audio/ogg" => Some("audio/ogg"),
        "audio/mp4" => Some("audio/mp4"),
        "audio/aac" => Some("audio/aac"),
        "audio/aiff" => Some("audio/aiff"),
        "audio/x-ms-wma" => Some("audio/x-ms-wma"),
        _ => None,
    }
}

fn name(file: Uuid, attempt: Uuid, extension: &str) -> String {
    format!("{file}.{attempt}.{extension}")
}

fn private_file(file: &File, length: Option<u64>) -> Result<()> {
    let metadata = file.metadata().map_err(|_| unavailable())?;
    if private_metadata(&metadata, length) {
        Ok(())
    } else {
        Err(unavailable())
    }
}

fn private_metadata(metadata: &std::fs::Metadata, length: Option<u64>) -> bool {
    if !metadata.is_file() || length.is_some_and(|length| metadata.len() != length) {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if metadata.uid() != unsafe { libc::geteuid() }
            || metadata.mode() & 0o077 != 0
            || metadata.nlink() != 1
        {
            return false;
        }
    }
    true
}

fn unavailable() -> Error {
    Error::Internal("Private download storage is unavailable".into())
}

fn storage_failure(error: &std::io::Error) -> Error {
    #[cfg(unix)]
    if matches!(error.raw_os_error(), Some(libc::ENOSPC | libc::EDQUOT)) {
        return Error::InsufficientStorage;
    }
    #[cfg(not(unix))]
    let _ = error;
    unavailable()
}
