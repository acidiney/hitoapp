use crate::{StateConfig, StateError, StateMode};
use cap_fs_ext::{DirExt, FollowSymlinks, OpenOptionsFollowExt};
use cap_std::fs::{Dir, OpenOptions};
use std::{
    fs::File,
    io::{self, Read},
    path::{Component, Path, PathBuf},
};
use zeroize::Zeroizing;

const DATABASE: &str = "preparation.sqlite";
const MAX_DATABASE: u64 = 16 * 1024 * 1024;
const MAX_WAL: u64 = 17 * 1024 * 1024;
const MAX_SHM: u64 = 64 * 1024;
const MAX_OWNED: u64 = 40 * 1024 * 1024;

pub(super) struct Files {
    directory: Dir,
    #[cfg(not(target_os = "linux"))]
    path: PathBuf,
    database: File,
    _lock: File,
}

impl Files {
    pub(super) fn acquire(config: &StateConfig) -> Result<(Self, Zeroizing<[u8; 32]>), StateError> {
        let path = normal_absolute(&config.directory)?;
        let key_path = normal_absolute(&config.key_file)?;
        if key_path.starts_with(&path) {
            return Err(StateError::InvalidConfiguration);
        }
        let key = read_key(&key_path)?;
        let parent = path.parent().ok_or(StateError::InvalidConfiguration)?;
        let name = path.file_name().ok_or(StateError::InvalidConfiguration)?;
        let parent = open_directory(parent)?;
        if matches!(config.mode, StateMode::CreateNew) {
            let mut builder = cap_std::fs::DirBuilder::new();
            #[cfg(unix)]
            {
                use cap_std::fs::DirBuilderExt;
                builder.mode(0o700);
            }
            match parent.create_dir_with(name, &builder) {
                Ok(()) => sync_directory(&parent)?,
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                Err(_) => return Err(StateError::Unavailable),
            }
        }
        let directory = parent.open_dir_nofollow(name).map_err(unavailable)?;
        private(
            &directory
                .try_clone()
                .map_err(unavailable)?
                .into_std_file()
                .metadata()
                .map_err(unavailable)?,
            true,
        )?;
        let lock = open_private(&directory, "owner.lock", true, false)?;
        lock.try_lock().map_err(|_| StateError::AlreadyOwned)?;
        let database = open_private(
            &directory,
            DATABASE,
            false,
            matches!(config.mode, StateMode::CreateNew),
        )?;
        let files = Self {
            directory,
            #[cfg(not(target_os = "linux"))]
            path,
            database,
            _lock: lock,
        };
        files.check()?;
        files.sync_directory()?;
        Ok((files, key))
    }

    pub(super) fn database_path(&self) -> PathBuf {
        #[cfg(target_os = "linux")]
        {
            use std::os::fd::AsRawFd;
            PathBuf::from(format!(
                "/proc/self/fd/{}/{DATABASE}",
                self.directory.as_raw_fd()
            ))
        }
        #[cfg(not(target_os = "linux"))]
        self.path.join(DATABASE)
    }

    pub(super) fn sync_directory(&self) -> Result<(), StateError> {
        sync_directory(&self.directory)
    }

    pub(super) fn check(&self) -> Result<(), StateError> {
        let mut logical = 0_u64;
        let mut allocated = 0_u64;
        let mut found_database = false;
        for entry in self.directory.entries().map_err(unavailable)? {
            let entry = entry.map_err(unavailable)?;
            let name = entry.file_name();
            let maximum = match name.to_str() {
                Some("owner.lock") => 0,
                Some(DATABASE) => MAX_DATABASE,
                Some("preparation.sqlite-wal") => MAX_WAL,
                Some("preparation.sqlite-shm") => MAX_SHM,
                _ => return Err(StateError::InvalidState),
            };
            let file = open_private(&self.directory, &name, false, false)?;
            let metadata = file.metadata().map_err(unavailable)?;
            if metadata.len() > maximum {
                return Err(StateError::InvalidState);
            }
            logical = logical
                .checked_add(metadata.len())
                .ok_or(StateError::InvalidState)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::MetadataExt;
                allocated = allocated
                    .checked_add(
                        metadata
                            .blocks()
                            .checked_mul(512)
                            .ok_or(StateError::InvalidState)?,
                    )
                    .ok_or(StateError::InvalidState)?;
                if name == DATABASE {
                    found_database = true;
                    let original = self.database.metadata().map_err(unavailable)?;
                    if metadata.dev() != original.dev() || metadata.ino() != original.ino() {
                        return Err(StateError::InvalidState);
                    }
                }
            }
        }
        if !found_database || logical > MAX_OWNED || allocated > MAX_OWNED {
            return Err(StateError::InvalidState);
        }
        Ok(())
    }
}

fn sync_directory(directory: &Dir) -> Result<(), StateError> {
    let mut options = OpenOptions::new();
    options.read(true).follow(FollowSymlinks::No);
    #[cfg(unix)]
    {
        use cap_std::fs::OpenOptionsExt;
        options.custom_flags(libc::O_DIRECTORY);
    }
    directory
        .open_with(".", &options)
        .and_then(|directory| directory.into_std().sync_all())
        .map_err(unavailable)
}

fn read_key(path: &Path) -> Result<Zeroizing<[u8; 32]>, StateError> {
    let directory = open_directory(path.parent().ok_or(StateError::InvalidConfiguration)?)?;
    let mut options = OpenOptions::new();
    options.read(true).follow(FollowSymlinks::No);
    #[cfg(unix)]
    {
        use cap_std::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NONBLOCK);
    }
    let mut file = directory
        .open_with(
            path.file_name().ok_or(StateError::InvalidConfiguration)?,
            &options,
        )
        .map_err(unavailable)?
        .into_std();
    private(&file.metadata().map_err(unavailable)?, false)?;
    if file.metadata().map_err(unavailable)?.len() != 32 {
        return Err(StateError::InvalidConfiguration);
    }
    let mut key = Zeroizing::new([0_u8; 32]);
    file.read_exact(key.as_mut()).map_err(unavailable)?;
    let mut extra = [0_u8; 1];
    if file.read(&mut extra).map_err(unavailable)? != 0 || key.iter().all(|byte| *byte == 0) {
        return Err(StateError::InvalidConfiguration);
    }
    Ok(key)
}

fn normal_absolute(path: &Path) -> Result<PathBuf, StateError> {
    let path = std::path::absolute(path).map_err(unavailable)?;
    if path.file_name().is_none()
        || path
            .components()
            .any(|part| !matches!(part, Component::RootDir | Component::Normal(_)))
    {
        return Err(StateError::InvalidConfiguration);
    }
    Ok(path)
}

fn open_directory(path: &Path) -> Result<Dir, StateError> {
    let mut directory =
        Dir::open_ambient_dir("/", cap_std::ambient_authority()).map_err(unavailable)?;
    ancestor(&directory)?;
    for part in path.components() {
        match part {
            Component::RootDir => {}
            Component::Normal(name) => {
                directory = directory.open_dir_nofollow(name).map_err(unavailable)?;
                ancestor(&directory)?;
            }
            _ => return Err(StateError::InvalidConfiguration),
        }
    }
    Ok(directory)
}

fn open_private(
    directory: &Dir,
    name: impl AsRef<Path>,
    create: bool,
    create_new: bool,
) -> Result<File, StateError> {
    let mut options = OpenOptions::new();
    options
        .read(true)
        .write(true)
        .create(create)
        .create_new(create_new)
        .follow(FollowSymlinks::No);
    #[cfg(unix)]
    {
        use cap_std::fs::OpenOptionsExt;
        options.mode(0o600).custom_flags(libc::O_NONBLOCK);
    }
    let file = directory
        .open_with(name, &options)
        .map_err(unavailable)?
        .into_std();
    private(&file.metadata().map_err(unavailable)?, false)?;
    Ok(file)
}

fn private(metadata: &std::fs::Metadata, directory: bool) -> Result<(), StateError> {
    if metadata.is_dir() != directory || (!directory && !metadata.is_file()) {
        return Err(StateError::InvalidState);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if metadata.uid() != unsafe { libc::geteuid() }
            || metadata.mode() & 0o7777 != if directory { 0o700 } else { 0o600 }
            || (!directory && metadata.nlink() != 1)
        {
            return Err(StateError::InvalidState);
        }
        Ok(())
    }
    #[cfg(not(unix))]
    Err(StateError::InvalidConfiguration)
}

fn ancestor(directory: &Dir) -> Result<(), StateError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let metadata = directory
            .try_clone()
            .map_err(unavailable)?
            .into_std_file()
            .metadata()
            .map_err(unavailable)?;
        if !metadata.is_dir()
            || (metadata.uid() != 0 && metadata.uid() != unsafe { libc::geteuid() })
            || (metadata.mode() & 0o022 != 0 && metadata.mode() & 0o1000 == 0)
        {
            return Err(StateError::InvalidState);
        }
        Ok(())
    }
    #[cfg(not(unix))]
    Err(StateError::InvalidConfiguration)
}

fn unavailable(_: io::Error) -> StateError {
    StateError::Unavailable
}
