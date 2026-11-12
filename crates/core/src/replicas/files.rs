mod retirement;
pub(super) use retirement::remove;

use crate::{Error, Result};
use cap_fs_ext::{DirExt, FollowSymlinks, OpenOptionsFollowExt};
use cap_std::fs::{Dir, DirBuilder, OpenOptions};
use std::{
    fs::File,
    io::{Read, Write},
    path::{Component, Path, PathBuf},
};
use uuid::Uuid;

pub(super) struct ProfileFiles {
    pub path: PathBuf,
    pub _lock: File,
}

pub(super) fn unavailable() -> Error {
    Error::Internal("Private replica storage is unavailable".into())
}

fn root(path: &Path) -> Result<Dir> {
    let path = if path.is_absolute() {
        path.to_owned()
    } else {
        std::env::current_dir()
            .map_err(|_| unavailable())?
            .join(path)
    };
    let parent = path.parent().ok_or_else(unavailable)?;
    let mut anchor = PathBuf::new();
    let mut directory: Option<Dir> = None;
    for component in parent.components() {
        match component {
            Component::Prefix(_) | Component::RootDir => anchor.push(component.as_os_str()),
            Component::CurDir => {}
            Component::ParentDir => return Err(unavailable()),
            Component::Normal(name) => {
                let current = match directory.take() {
                    Some(current) => current,
                    None => Dir::open_ambient_dir(&anchor, cap_std::ambient_authority())
                        .map_err(|_| unavailable())?,
                };
                directory = Some(current.open_dir_nofollow(name).map_err(|_| unavailable())?);
                anchor.push(name);
            }
        }
    }
    let parent = match directory {
        Some(parent) => parent,
        None => Dir::open_ambient_dir(&anchor, cap_std::ambient_authority())
            .map_err(|_| unavailable())?,
    };
    let name = path.file_name().ok_or_else(unavailable)?;
    create_directory(&parent, name)?;
    let root = parent.open_dir_nofollow(name).map_err(|_| unavailable())?;
    check_directory(&root)?;
    Ok(root)
}

fn check_directory(directory: &Dir) -> Result<()> {
    let metadata = directory.dir_metadata().map_err(|_| unavailable())?;
    if !metadata.is_dir() || metadata.is_symlink() {
        return Err(unavailable());
    }
    #[cfg(unix)]
    {
        use cap_std::fs::MetadataExt;
        if metadata.uid() != unsafe { libc::geteuid() } || metadata.mode() & 0o077 != 0 {
            return Err(unavailable());
        }
    }
    #[cfg(not(unix))]
    {
        return Err(unavailable());
    }
    Ok(())
}

fn create_directory(parent: &Dir, name: impl AsRef<Path>) -> Result<()> {
    let mut builder = DirBuilder::new();
    #[cfg(unix)]
    {
        use cap_std::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    match parent.create_dir_with(name, &builder) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => Ok(()),
        Err(_) => Err(unavailable()),
    }
}

fn private_file(directory: &Dir, name: &str, create: bool) -> Result<File> {
    let mut options = OpenOptions::new();
    options
        .read(true)
        .write(true)
        .create(create)
        .follow(FollowSymlinks::No);
    #[cfg(unix)]
    {
        use cap_std::fs::OpenOptionsExt;
        options.mode(0o600).custom_flags(libc::O_NONBLOCK);
    }
    let file = directory
        .open_with(name, &options)
        .map_err(|_| unavailable())?
        .into_std();
    if !crate::private_file::is_private(&file.metadata().map_err(|_| unavailable())?, &[0o600]) {
        return Err(unavailable());
    }
    Ok(file)
}

fn marker(directory: &Dir, id: Uuid, create: bool) -> Result<()> {
    let expected = format!("hito-replica-v1\n{id}\n");
    let mut marker = private_file(directory, "owner", create)?;
    if marker.metadata().map_err(|_| unavailable())?.len() == 0 && create {
        marker
            .write_all(expected.as_bytes())
            .map_err(|_| unavailable())?;
        marker.sync_all().map_err(|_| unavailable())?;
    } else {
        let mut bytes = Vec::new();
        marker
            .take(128)
            .read_to_end(&mut bytes)
            .map_err(|_| unavailable())?;
        if bytes != expected.as_bytes() {
            return Err(unavailable());
        }
    }
    Ok(())
}

pub(super) fn open(path: &Path, id: Uuid) -> Result<ProfileFiles> {
    let root = root(path)?;
    let name = id.to_string();
    create_directory(&root, &name)?;
    let directory = root.open_dir_nofollow(&name).map_err(|_| unavailable())?;
    check_directory(&directory)?;
    let lock = private_file(&directory, "profile.lock", true)?;
    lock.try_lock()
        .map_err(|_| Error::Conflict("This replica is already open".into()))?;
    marker(&directory, id, true)?;
    drop(private_file(&directory, "hito.db", true)?);
    for name in ["hito.db-wal", "hito.db-shm", "hito.db-journal"] {
        if directory.symlink_metadata(name).is_ok() {
            drop(private_file(&directory, name, false)?);
        }
    }
    Ok(ProfileFiles {
        path: path.join(name).join("hito.db"),
        _lock: lock,
    })
}
