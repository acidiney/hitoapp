use cap_std::fs::Dir;
use std::{
    io,
    path::{Path, PathBuf},
};

#[cfg(unix)]
pub(super) fn open(path: &Path, initialize: bool) -> io::Result<(Dir, PathBuf)> {
    use cap_fs_ext::DirExt;
    use cap_std::fs::{DirBuilder, DirBuilderExt};

    let absolute = std::path::absolute(path)?;
    let (Some(parent), Some(name)) = (absolute.parent(), absolute.file_name()) else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Select a folder state directory with a normal final component",
        ));
    };
    let uid = unsafe { libc::geteuid() };
    let (directory, parent) = open_parent(parent, uid)?;
    if initialize {
        let mut builder = DirBuilder::new();
        builder.mode(0o700);
        match directory.create_dir_with(name, &builder) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error),
        }
    }
    let directory = directory.open_dir_nofollow(name)?;
    super::validate_private(&directory.try_clone()?.into_std_file().metadata()?, true)?;
    Ok((directory, parent.join(name)))
}

#[cfg(unix)]
fn open_parent(path: &Path, uid: libc::uid_t) -> io::Result<(Dir, PathBuf)> {
    use cap_fs_ext::DirExt;
    use cap_std::fs::MetadataExt;
    use std::{collections::VecDeque, path::Component};

    let mut directory = Dir::open_ambient_dir("/", cap_std::ambient_authority())?;
    validate_ancestor(&directory, uid)?;
    let mut parents = Vec::new();
    let mut resolved = PathBuf::from("/");
    let mut pending = VecDeque::from([path.to_owned()]);
    let mut aliases = 0;
    while let Some(fragment) = pending.pop_front() {
        let mut components = fragment.components();
        while let Some(component) = components.next() {
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
                    let metadata = directory.symlink_metadata(name)?;
                    if metadata.file_type().is_symlink() {
                        if metadata.uid() != 0 && metadata.uid() != uid {
                            return Err(io::Error::new(
                                io::ErrorKind::PermissionDenied,
                                "Folder state aliases must be owned and protected",
                            ));
                        }
                        if aliases == 40 {
                            return Err(io::Error::new(
                                io::ErrorKind::InvalidInput,
                                "Folder state alias limit exceeded",
                            ));
                        }
                        aliases += 1;
                        let target = directory.read_link_contents(name)?;
                        if target.as_os_str().is_empty() {
                            return Err(io::Error::new(
                                io::ErrorKind::InvalidInput,
                                "Invalid folder state alias",
                            ));
                        }
                        pending.push_front(components.as_path().to_owned());
                        pending.push_front(target);
                        break;
                    }
                    let child = directory.open_dir_nofollow(name)?;
                    validate_ancestor(&child, uid)?;
                    parents.push(directory);
                    directory = child;
                    resolved.push(name);
                }
                _ => {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "Invalid folder state ancestry",
                    ));
                }
            }
        }
    }
    Ok((directory, resolved))
}

#[cfg(unix)]
fn validate_ancestor(directory: &Dir, uid: libc::uid_t) -> io::Result<()> {
    use std::os::unix::fs::MetadataExt;

    let metadata = directory.try_clone()?.into_std_file().metadata()?;
    let trusted_owner = metadata.uid() == 0 || metadata.uid() == uid;
    let protected_entries = metadata.mode() & 0o022 == 0 || metadata.mode() & 0o1000 != 0;
    if !metadata.is_dir() || !trusted_owner || !protected_entries {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "Folder state ancestry must be owned and protected",
        ));
    }
    Ok(())
}

#[cfg(not(unix))]
pub(super) fn open(_: &Path, _: bool) -> io::Result<(Dir, PathBuf)> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "Persistent folder state requires Unix ownership",
    ))
}
