use cap_fs_ext::{FollowSymlinks, OpenOptionsFollowExt};
use cap_std::fs::{Dir, OpenOptions};
use std::{
    fs::File,
    io,
    path::{Path, PathBuf},
    sync::Arc,
};
use tokio::sync::Semaphore;

#[derive(Clone)]
pub(crate) struct Root {
    path: PathBuf,
    pub directory: Arc<Dir>,
    pub(super) slots: Arc<Semaphore>,
    pub(super) walk_slots: Arc<Semaphore>,
}

impl Root {
    #[cfg(unix)]
    pub(super) fn from_owned_dir(path: PathBuf, directory: Dir, slots: Arc<Semaphore>) -> Self {
        Self {
            path,
            directory: Arc::new(directory),
            slots,
            walk_slots: Arc::new(Semaphore::new(1)),
        }
    }

    pub async fn open(
        path: &Path,
        slots: &Arc<Semaphore>,
        walk_slots: &Arc<Semaphore>,
        components: &[String],
    ) -> crate::Result<Self> {
        let path = path.to_owned();
        let components = components.to_owned();
        let root_slots = slots.clone();
        let walk_slots = walk_slots.clone();
        super::blocking::run(slots.clone(), move |cancelled| {
            let path = path.canonicalize().map_err(|_| unavailable())?;
            super::blocking::check(cancelled)?;
            let mut directory = Dir::open_ambient_dir(&path, cap_std::ambient_authority())
                .map_err(|_| unavailable())?;
            let mut selected = path;
            for component in components {
                super::blocking::check(cancelled)?;
                if directory
                    .symlink_metadata(&component)
                    .map_err(|_| unavailable())?
                    .file_type()
                    .is_symlink()
                {
                    return Err(unavailable());
                }
                directory = directory.open_dir(&component).map_err(|_| unavailable())?;
                selected.push(component);
            }
            super::blocking::check(cancelled)?;
            Ok(Self {
                path: selected,
                directory: Arc::new(directory),
                slots: root_slots,
                walk_slots,
            })
        })
        .await
        .map_err(|_| unavailable())
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn open_file(&self, path: &Path) -> io::Result<File> {
        let relative = path
            .strip_prefix(&self.path)
            .map_err(|_| io::ErrorKind::PermissionDenied)?;
        let mut options = OpenOptions::new();
        options.read(true).follow(FollowSymlinks::No);
        #[cfg(unix)]
        {
            use cap_std::fs::OpenOptionsExt;
            options.custom_flags(libc::O_NONBLOCK);
        }
        let file = self.directory.open_with(relative, &options)?.into_std();
        if !file.metadata()?.is_file() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Media requires a regular file",
            ));
        }
        Ok(file)
    }
}

fn unavailable() -> crate::Error {
    crate::Error::Internal("Music folder is unavailable".into())
}
