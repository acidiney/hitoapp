use super::{
    ManagedFileLocation, ManagedFiles, PendingManagedFile, Root, VerifiedManagedFile, blocking,
    name, private_file, unavailable,
};
use crate::{Error, Result};
#[cfg(unix)]
use cap_fs_ext::{FollowSymlinks, OpenOptionsFollowExt};
#[cfg(unix)]
use cap_std::fs::OpenOptions;
use std::io::Write;

impl ManagedFiles {
    #[tracing::instrument(level = "debug", skip_all, err(Debug), fields(file_id = %pending.file_id))]
    pub(crate) async fn verify(&self, pending: PendingManagedFile) -> Result<VerifiedManagedFile> {
        let file = pending.file.clone();
        let writer = pending.writer.clone();
        blocking::run(pending.root.slots.clone(), move |_| {
            let _writer = writer;
            let mut file = file.lock().map_err(|_| unavailable())?;
            file.flush()
                .and_then(|()| file.sync_all())
                .map_err(|_| unavailable())
        })
        .await?;
        if pending.length == 0 {
            return Err(Error::Invalid("Downloaded audio is empty".into()));
        }
        let path = pending
            .root
            .path()
            .join(name(pending.file_id, pending.attempt_id, "part"));
        let (inspected, mime) = self
            .inspector
            .scoped(pending.writer.inspection())
            .inspect_download(&pending.root, &path)
            .await?;
        if inspected.size < 0 || inspected.size as u64 != pending.length {
            return Err(Error::Conflict(
                "Downloaded audio changed during verification".into(),
            ));
        }
        tracing::debug!(
            event = "download_audio_verified",
            byte_length = pending.length
        );
        Ok(VerifiedManagedFile {
            location: ManagedFileLocation {
                file_id: pending.file_id,
                attempt_id: pending.attempt_id,
                generation: 1,
                checksum: inspected.checksum,
                byte_length: pending.length,
                mime,
            },
            root: pending.root,
            writer: pending.writer,
        })
    }

    #[tracing::instrument(level = "debug", skip_all, err(Debug), fields(file_id = %verified.location.file_id))]
    pub(crate) async fn publish(
        &self,
        verified: VerifiedManagedFile,
    ) -> Result<ManagedFileLocation> {
        blocking::run(verified.root.slots.clone(), move |cancelled| {
            let writer = verified.writer;
            writer.check()?;
            let location = verified.location;
            let staging = name(location.file_id, location.attempt_id, "part");
            let final_name = name(location.file_id, location.attempt_id, "media");
            let file = verified
                .root
                .open_file(&verified.root.path().join(&staging))
                .map_err(|_| unavailable())?;
            private_file(&file, Some(location.byte_length))?;
            blocking::check(cancelled)?;
            verified
                .root
                .directory
                .hard_link(&staging, &verified.root.directory, &final_name)
                .map_err(|_| unavailable())?;
            verified
                .root
                .directory
                .remove_file(staging)
                .map_err(|_| unavailable())?;
            sync_directory(&verified.root)?;
            tracing::debug!(event = "download_file_published");
            Ok(location)
        })
        .await
    }
}

pub(super) fn sync_directory(root: &Root) -> Result<()> {
    #[cfg(unix)]
    {
        use cap_std::fs::OpenOptionsExt;
        let mut options = OpenOptions::new();
        options
            .read(true)
            .follow(FollowSymlinks::No)
            .custom_flags(libc::O_DIRECTORY);
        root.directory
            .open_with(".", &options)
            .and_then(|directory| directory.into_std().sync_all())
            .map_err(|error| {
                tracing::debug!(
                    event = "download_directory_sync_failed",
                    error_kind = ?error.kind(),
                    os_error = error.raw_os_error(),
                    "Download directory sync failed"
                );
                unavailable()
            })?;
    }
    #[cfg(not(unix))]
    let _ = root;
    Ok(())
}
