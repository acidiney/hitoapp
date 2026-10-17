use cap_std::fs::Dir;
use std::path::{Path, PathBuf};

use crate::Error;

#[cfg(unix)]
pub(super) fn open_parent(path: &Path) -> Result<(Dir, PathBuf), Error> {
    use cap_fs_ext::DirExt;
    use cap_std::fs::MetadataExt;
    use std::{collections::VecDeque, path::Component};

    if !path.is_absolute() || path.as_os_str().as_encoded_bytes().len() > 4096 {
        return Err(Error::StateUnavailable);
    }
    let uid = unsafe { libc::geteuid() };
    let mut directory = Dir::open_ambient_dir("/", cap_std::ambient_authority())
        .map_err(|_| Error::StateUnavailable)?;
    admit_ancestor(&directory, uid)?;
    let mut parents = Vec::new();
    let mut resolved = PathBuf::from("/");
    let mut pending = VecDeque::from([path.to_owned()]);
    let mut aliases = 0;
    let mut visited = 0;
    while let Some(fragment) = pending.pop_front() {
        let mut components = fragment.components();
        while let Some(component) = components.next() {
            visited += 1;
            if visited > 256 {
                return Err(Error::StateUnavailable);
            }
            match component {
                Component::RootDir => {
                    while let Some(parent) = parents.pop() {
                        directory = parent;
                    }
                    resolved = PathBuf::from("/");
                }
                Component::CurDir => {}
                Component::ParentDir => {
                    if let Some(parent) = parents.pop() {
                        directory = parent;
                        resolved.pop();
                    }
                }
                Component::Normal(name) => {
                    let metadata = directory
                        .symlink_metadata(name)
                        .map_err(|_| Error::StateUnavailable)?;
                    if metadata.is_symlink() {
                        if (metadata.uid() != 0 && metadata.uid() != uid) || aliases >= 40 {
                            return Err(Error::StateUnavailable);
                        }
                        let target = directory
                            .read_link_contents(name)
                            .map_err(|_| Error::StateUnavailable)?;
                        if target.as_os_str().is_empty()
                            || target.as_os_str().as_encoded_bytes().len() > 4096
                        {
                            return Err(Error::StateUnavailable);
                        }
                        aliases += 1;
                        pending.push_front(components.as_path().to_owned());
                        pending.push_front(target);
                        break;
                    }
                    let child = directory
                        .open_dir_nofollow(name)
                        .map_err(|_| Error::StateUnavailable)?;
                    admit_ancestor(&child, uid)?;
                    parents.push(directory);
                    directory = child;
                    resolved.push(name);
                }
                _ => return Err(Error::StateUnavailable),
            }
        }
    }
    Ok((directory, resolved))
}

#[cfg(unix)]
fn admit_ancestor(directory: &Dir, uid: libc::uid_t) -> Result<(), Error> {
    use cap_std::fs::MetadataExt;

    let metadata = directory
        .dir_metadata()
        .map_err(|_| Error::StateUnavailable)?;
    let owner_allowed = metadata.uid() == 0 || metadata.uid() == uid;
    let entries_protected = metadata.mode() & 0o022 == 0 || metadata.mode() & 0o1000 != 0;
    if !metadata.is_dir() || !owner_allowed || !entries_protected {
        return Err(Error::StateUnavailable);
    }
    Ok(())
}

#[cfg(not(unix))]
pub(super) fn open_parent(_: &Path) -> Result<(Dir, PathBuf), Error> {
    Err(Error::StateUnavailable)
}
