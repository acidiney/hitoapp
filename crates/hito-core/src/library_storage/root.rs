use cap_fs_ext::{FollowSymlinks, OpenOptionsFollowExt};
use cap_std::fs::{Dir, OpenOptions};
use std::{
    fs::File,
    io,
    path::{Path, PathBuf},
    sync::Arc,
};

#[derive(Clone)]
pub(super) struct Root {
    path: PathBuf,
    pub directory: Arc<Dir>,
}

impl Root {
    pub async fn open(path: &Path) -> crate::Result<Self> {
        let path = path.to_owned();
        tokio::task::spawn_blocking(move || {
            let path = path.canonicalize()?;
            let directory = Dir::open_ambient_dir(&path, cap_std::ambient_authority())?;
            Ok::<_, io::Error>(Self {
                path,
                directory: Arc::new(directory),
            })
        })
        .await
        .map_err(|_| unavailable())?
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
