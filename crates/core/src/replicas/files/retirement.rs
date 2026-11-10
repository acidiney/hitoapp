use super::{check_directory, marker, private_file, root, unavailable};
use crate::Result;
use cap_fs_ext::{DirExt, FollowSymlinks, OpenOptionsFollowExt};
use cap_std::fs::{Dir, OpenOptions};
use std::path::Path;
use uuid::Uuid;

const DATA_FILES: [&str; 4] = ["hito.db", "hito.db-wal", "hito.db-shm", "hito.db-journal"];

fn contents(directory: &Dir) -> Result<Vec<String>> {
    let mut names = Vec::new();
    for entry in directory.entries().map_err(|_| unavailable())? {
        let name = entry
            .map_err(|_| unavailable())?
            .file_name()
            .into_string()
            .map_err(|_| unavailable())?;
        if names.len() == 6
            || (name != "owner" && name != "profile.lock" && !DATA_FILES.contains(&name.as_str()))
        {
            return Err(unavailable());
        }
        drop(private_file(directory, &name, false)?);
        names.push(name);
    }
    Ok(names)
}

fn sync_directory(directory: &Dir) -> Result<()> {
    #[cfg(unix)]
    {
        use cap_std::fs::OpenOptionsExt;
        let mut options = OpenOptions::new();
        options
            .read(true)
            .follow(FollowSymlinks::No)
            .custom_flags(libc::O_DIRECTORY);
        directory
            .open_with(".", &options)
            .and_then(|file| file.into_std().sync_all())
            .map_err(|_| unavailable())?;
        Ok(())
    }
    #[cfg(not(unix))]
    {
        let _ = directory;
        Err(unavailable())
    }
}

fn finish(root: &Dir, name: &str, directory: Dir) -> Result<()> {
    sync_directory(&directory)?;
    drop(directory);
    root.remove_dir(name).map_err(|_| unavailable())?;
    sync_directory(root)
}

pub(in crate::replicas) fn remove(path: &Path, id: Uuid) -> Result<()> {
    match std::fs::symlink_metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(_) => return Err(unavailable()),
        Ok(_) => {}
    }
    let root = root(path)?;
    let name = id.to_string();
    let directory = match root.open_dir_nofollow(&name) {
        Ok(directory) => directory,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return sync_directory(&root),
        Err(_) => return Err(unavailable()),
    };
    check_directory(&directory)?;
    let names = contents(&directory)?;
    if names.is_empty() {
        return finish(&root, &name, directory);
    }
    if !names.iter().any(|name| name == "profile.lock") {
        return Err(unavailable());
    }
    let lock = private_file(&directory, "profile.lock", false)?;
    lock.try_lock().map_err(|_| unavailable())?;
    if lock.metadata().map_err(|_| unavailable())?.len() != 0 {
        return Err(unavailable());
    }
    if names.iter().any(|name| name == "owner") {
        marker(&directory, id, false)?;
        for name in DATA_FILES {
            if names.iter().any(|present| present == name) {
                directory.remove_file(name).map_err(|_| unavailable())?;
            }
        }
        sync_directory(&directory)?;
        let remaining = contents(&directory)?;
        if remaining.len() != 2
            || !remaining.iter().any(|name| name == "owner")
            || !remaining.iter().any(|name| name == "profile.lock")
        {
            return Err(unavailable());
        }
        directory.remove_file("owner").map_err(|_| unavailable())?;
        sync_directory(&directory)?;
    } else if names.len() != 1 {
        return Err(unavailable());
    }
    directory
        .remove_file("profile.lock")
        .map_err(|_| unavailable())?;
    drop(lock);
    finish(&root, &name, directory)
}
