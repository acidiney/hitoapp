use super::unavailable;
use crate::Result;
use cap_fs_ext::DirExt;
use cap_std::fs::{Dir, DirBuilder, Metadata};
use std::path::{Component, Path, PathBuf};

pub(super) fn open(database: &Path) -> Result<(PathBuf, Dir)> {
    let database = if database.is_absolute() {
        database.to_owned()
    } else {
        std::env::current_dir()
            .map_err(|_| unavailable())?
            .join(database)
    };
    let parent = database.parent().ok_or_else(unavailable)?;
    let mut anchor = PathBuf::new();
    let mut directory = None;
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
    let parent_dir = match directory {
        Some(directory) => directory,
        None => Dir::open_ambient_dir(&anchor, cap_std::ambient_authority())
            .map_err(|_| unavailable())?,
    };
    owned(
        &parent_dir.dir_metadata().map_err(|_| unavailable())?,
        false,
    )?;
    let db_metadata = parent_dir
        .symlink_metadata(database.file_name().ok_or_else(unavailable)?)
        .map_err(|_| unavailable())?;
    if !db_metadata.is_file() || db_metadata.is_symlink() {
        return Err(unavailable());
    }
    let path = database.with_extension("downloads");
    let name = path.file_name().ok_or_else(unavailable)?;
    let mut builder = DirBuilder::new();
    #[cfg(unix)]
    {
        use cap_std::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    match parent_dir.create_dir_with(name, &builder) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(_) => return Err(unavailable()),
    }
    let directory = parent_dir
        .open_dir_nofollow(name)
        .map_err(|_| unavailable())?;
    owned(&directory.dir_metadata().map_err(|_| unavailable())?, true)?;
    Ok((path, directory))
}

fn owned(metadata: &Metadata, private: bool) -> Result<()> {
    if !metadata.is_dir() || metadata.is_symlink() {
        return Err(unavailable());
    }
    #[cfg(unix)]
    {
        use cap_std::fs::MetadataExt;
        if metadata.uid() != unsafe { libc::geteuid() }
            || metadata.mode() & (if private { 0o077 } else { 0o022 }) != 0
        {
            return Err(unavailable());
        }
    }
    Ok(())
}
