use super::{
    MAX_MANAGED_BYTES, ManagedFileLocation, ManagedFiles, blocking, name, private_metadata,
    unavailable,
};
use crate::{Error, Result, catalog::AudioFile};

impl ManagedFiles {
    #[tracing::instrument(level = "debug", skip_all, err(Debug), fields(file_id = %location.file_id))]
    pub(crate) async fn available(&self, location: &ManagedFileLocation) -> Result<bool> {
        let root = self.root()?.clone();
        if location.generation != 1
            || location.byte_length == 0
            || location.byte_length > MAX_MANAGED_BYTES
        {
            return Ok(false);
        }
        let path = root
            .path()
            .join(name(location.file_id, location.attempt_id, "media"));
        let length = location.byte_length;
        blocking::run(root.slots.clone(), move |_| {
            let file = match root.open_file(&path) {
                Ok(file) => file,
                Err(error) if missing_or_invalid(&error) => return Ok(false),
                Err(_) => return Err(unavailable()),
            };
            Ok(private_metadata(
                &file.metadata().map_err(|_| unavailable())?,
                Some(length),
            ))
        })
        .await
    }

    #[tracing::instrument(level = "debug", skip_all, err(Debug), fields(file_id = %location.file_id))]
    pub(crate) async fn open_file(&self, location: &ManagedFileLocation) -> Result<AudioFile> {
        let root = self.root()?.clone();
        if !self.available(location).await? {
            return Err(Error::NotFound);
        }
        let path = root
            .path()
            .join(name(location.file_id, location.attempt_id, "media"));
        match self.inspector.checksum(&root, &path).await {
            Ok(checksum) if checksum == location.checksum => {}
            Ok(_) | Err(Error::Conflict(_)) => return Err(Error::NotFound),
            Err(error) => {
                return if self.available(location).await? {
                    Err(error)
                } else {
                    Err(Error::NotFound)
                };
            }
        }
        let length = location.byte_length;
        let content_type = location.mime;
        blocking::run(root.slots.clone(), move |_| {
            let file = root.open_file(&path).map_err(|error| {
                if missing_or_invalid(&error) {
                    Error::NotFound
                } else {
                    unavailable()
                }
            })?;
            if !private_metadata(&file.metadata().map_err(|_| unavailable())?, Some(length)) {
                return Err(Error::NotFound);
            }
            Ok(AudioFile {
                file,
                length,
                content_type,
            })
        })
        .await
    }
}

fn missing_or_invalid(error: &std::io::Error) -> bool {
    if matches!(
        error.kind(),
        std::io::ErrorKind::NotFound | std::io::ErrorKind::InvalidInput
    ) {
        return true;
    }
    #[cfg(unix)]
    if error.raw_os_error() == Some(libc::ELOOP) {
        return true;
    }
    false
}
