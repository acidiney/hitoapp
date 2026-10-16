use cap_std::fs::Dir;
use std::{
    io,
    path::{Path, PathBuf},
};

#[cfg(unix)]
pub(super) fn open(path: &Path, initialize: bool) -> io::Result<(Dir, PathBuf)> {
    use cap_fs_ext::DirExt;
    use cap_std::fs::{DirBuilder, DirBuilderExt};
    use std::path::Component;

    let absolute = std::path::absolute(path)?;
    let (Some(parent), Some(name)) = (absolute.parent(), absolute.file_name()) else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Select a folder state directory with a normal final component",
        ));
    };
    let parent = parent.canonicalize()?;
    let uid = unsafe { libc::geteuid() };
    let mut directory = Dir::open_ambient_dir("/", cap_std::ambient_authority())?;
    validate_ancestor(&directory, uid)?;
    for component in parent.components() {
        match component {
            Component::RootDir => {}
            Component::Normal(name) => {
                directory = directory.open_dir_nofollow(name)?;
                validate_ancestor(&directory, uid)?;
            }
            _ => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "Invalid folder state ancestry",
                ));
            }
        }
    }
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
