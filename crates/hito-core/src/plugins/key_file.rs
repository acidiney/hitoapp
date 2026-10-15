use rand::RngCore;
use std::{
    fs::{File, OpenOptions},
    io::{self, Read, Write},
    path::Path,
};
use zeroize::Zeroizing;

pub(super) fn load(path: &Path, allow_creation: bool) -> io::Result<Zeroizing<[u8; 32]>> {
    match read(path) {
        Ok(key) => Ok(key),
        Err(error) if error.kind() == io::ErrorKind::NotFound && allow_creation => {
            let parent = path
                .parent()
                .filter(|p| !p.as_os_str().is_empty())
                .unwrap_or(Path::new("."));
            let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
            let mut key = Zeroizing::new([0u8; 32]);
            rand::rngs::OsRng.fill_bytes(key.as_mut());
            temporary.write_all(key.as_ref())?;
            temporary.as_file().sync_all()?;
            match temporary.persist_noclobber(path) {
                Ok(_) => {
                    #[cfg(unix)]
                    File::open(parent)?.sync_all()?;
                    Ok(key)
                }
                Err(error) if error.error.kind() == io::ErrorKind::AlreadyExists => read(path),
                Err(error) => Err(error.error),
            }
        }
        Err(error) => Err(error),
    }
}

fn read(path: &Path) -> io::Result<Zeroizing<[u8; 32]>> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    let file = options.open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() || metadata.len() != 32 {
        return Err(io::Error::other("Invalid plugin credential key"));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o077 != 0 {
            return Err(io::Error::other("Plugin credential key is not private"));
        }
    }
    let mut bytes = Zeroizing::new(Vec::new());
    file.take(33).read_to_end(&mut bytes)?;
    let key: [u8; 32] = bytes
        .as_slice()
        .try_into()
        .map_err(|_| io::Error::other("Invalid plugin credential key"))?;
    Ok(Zeroizing::new(key))
}
