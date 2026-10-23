use crate::{Error, Result};
use std::{
    fs::{File, OpenOptions},
    path::Path,
};

pub(super) fn claim(data: &Path) -> Result<File> {
    if data
        .symlink_metadata()
        .is_ok_and(|metadata| metadata.file_type().is_symlink() || !metadata.is_dir())
    {
        return Err(Error::Storage);
    }
    let mut builder = std::fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(data).map_err(|_| Error::Storage)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(data, std::fs::Permissions::from_mode(0o700))
            .map_err(|_| Error::Storage)?;
    }
    let path = data.join("desktop.lock");
    if path
        .symlink_metadata()
        .is_ok_and(|metadata| metadata.file_type().is_symlink() || !metadata.is_file())
    {
        return Err(Error::Storage);
    }
    let mut options = OpenOptions::new();
    options.create(true).truncate(false).read(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let file = options.open(path).map_err(|_| Error::Storage)?;
    file.try_lock().map_err(|error| match error {
        std::fs::TryLockError::WouldBlock => Error::AlreadyRunning,
        std::fs::TryLockError::Error(_) => Error::Storage,
    })?;
    Ok(file)
}
