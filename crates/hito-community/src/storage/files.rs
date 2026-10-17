use cap_fs_ext::{DirExt, FollowSymlinks, OpenOptionsFollowExt};
use cap_std::fs::{Dir, OpenOptions};
use std::{
    fs::File,
    path::{Path, PathBuf},
};

use crate::Error;

pub(super) struct StateOwner {
    directory: Dir,
    #[cfg(not(target_os = "linux"))]
    path: PathBuf,
    _lock: File,
}

impl StateOwner {
    pub(super) fn acquire(path: &Path) -> Result<Self, Error> {
        let parent = path.parent().ok_or(Error::InvalidConfiguration)?;
        let name = path.file_name().ok_or(Error::InvalidConfiguration)?;
        let (parent, _resolved) = super::ancestry::open_parent(parent)?;
        let mut builder = cap_std::fs::DirBuilder::new();
        #[cfg(unix)]
        {
            use cap_std::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        match parent.create_dir_with(name, &builder) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(_) => return Err(Error::StateUnavailable),
        }
        let directory = parent
            .open_dir_nofollow(name)
            .map_err(|_| Error::StateUnavailable)?;
        private(
            &directory
                .try_clone()
                .map_err(|_| Error::StateUnavailable)?
                .into_std_file()
                .metadata()
                .map_err(|_| Error::StateUnavailable)?,
            true,
        )?;
        let lock = open_file(&directory, "owner.lock")?;
        lock.try_lock().map_err(|_| Error::StateOwned)?;
        open_file(&directory, "community.sqlite")?;
        for sidecar in ["community.sqlite-wal", "community.sqlite-shm"] {
            match directory.symlink_metadata(sidecar) {
                Ok(_) => {
                    open_file(&directory, sidecar)?;
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(_) => return Err(Error::StateUnavailable),
            }
        }
        Ok(Self {
            directory,
            #[cfg(not(target_os = "linux"))]
            path: _resolved.join(name),
            _lock: lock,
        })
    }

    pub(super) fn database_path(&self) -> PathBuf {
        #[cfg(target_os = "linux")]
        {
            use std::os::fd::AsRawFd;
            PathBuf::from(format!(
                "/proc/self/fd/{}/community.sqlite",
                self.directory.as_raw_fd()
            ))
        }
        #[cfg(not(target_os = "linux"))]
        self.path.join("community.sqlite")
    }
}

fn open_file(directory: &Dir, name: &str) -> Result<File, Error> {
    let mut options = OpenOptions::new();
    options
        .follow(FollowSymlinks::No)
        .read(true)
        .write(true)
        .create_new(true);
    #[cfg(unix)]
    {
        use cap_std::fs::OpenOptionsExt;
        options.mode(0o600).custom_flags(libc::O_NONBLOCK);
    }
    let file = match directory.open_with(name, &options) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            let metadata = directory
                .symlink_metadata(name)
                .map_err(|_| Error::StateUnavailable)?;
            if !metadata.is_file() || metadata.file_type().is_symlink() {
                return Err(Error::StateUnavailable);
            }
            options.create_new(false);
            directory
                .open_with(name, &options)
                .map_err(|_| Error::StateUnavailable)?
        }
        Err(_) => return Err(Error::StateUnavailable),
    }
    .into_std();
    private(
        &file.metadata().map_err(|_| Error::StateUnavailable)?,
        false,
    )?;
    Ok(file)
}

fn private(metadata: &std::fs::Metadata, directory: bool) -> Result<(), Error> {
    if metadata.is_dir() != directory || (!directory && !metadata.is_file()) {
        return Err(Error::StateUnavailable);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if metadata.uid() != unsafe { libc::geteuid() }
            || metadata.mode() & 0o7777 != if directory { 0o700 } else { 0o600 }
        {
            return Err(Error::StateUnavailable);
        }
    }
    #[cfg(not(unix))]
    return Err(Error::StateUnavailable);
    #[cfg(unix)]
    Ok(())
}
