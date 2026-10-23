use std::path::Path;
use zeroize::Zeroizing;

const MAX_TOKEN_BYTES: usize = 4098;

pub(super) fn read_token(path: &Path) -> Result<Zeroizing<String>, &'static str> {
    let bytes = read(path, MAX_TOKEN_BYTES)?;
    let token = std::str::from_utf8(&bytes).map_err(|_| "credential_file_invalid")?;
    let token = token
        .strip_suffix("\r\n")
        .or_else(|| token.strip_suffix('\n'))
        .unwrap_or(token);
    if token.is_empty()
        || token.len() > 4096
        || !token.bytes().all(|byte| (0x21..=0x7e).contains(&byte))
    {
        return Err("credential_file_invalid");
    }
    Ok(Zeroizing::new(token.to_owned()))
}

#[cfg(unix)]
pub(super) fn read(path: &Path, maximum: usize) -> Result<Zeroizing<Vec<u8>>, &'static str> {
    use std::{
        fs::OpenOptions,
        io::Read,
        os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
    };
    if !path.is_absolute() {
        return Err("operator_file_path_invalid");
    }
    let mut file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
        .map_err(|_| "operator_file_unavailable")?;
    let metadata = file.metadata().map_err(|_| "operator_file_unavailable")?;
    let private = |metadata: &std::fs::Metadata| {
        metadata.is_file()
            && metadata.nlink() == 1
            && metadata.uid() == unsafe { libc::geteuid() }
            && matches!(metadata.permissions().mode() & 0o7777, 0o400 | 0o600)
            && metadata.len() > 0
            && metadata.len() <= maximum as u64
    };
    if !private(&metadata) {
        return Err("operator_file_not_private");
    }
    let mut bytes = Zeroizing::new(Vec::with_capacity(metadata.len() as usize));
    (&mut file)
        .take(maximum as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "operator_file_invalid")?;
    let after = file.metadata().map_err(|_| "operator_file_unavailable")?;
    let identity = |metadata: &std::fs::Metadata| {
        (
            metadata.dev(),
            metadata.ino(),
            metadata.mode(),
            metadata.uid(),
            metadata.gid(),
            metadata.nlink(),
            metadata.len(),
            metadata.mtime(),
            metadata.mtime_nsec(),
            metadata.ctime(),
            metadata.ctime_nsec(),
        )
    };
    if !private(&after)
        || bytes.len() as u64 != metadata.len()
        || identity(&metadata) != identity(&after)
    {
        return Err("operator_file_changed");
    }
    Ok(bytes)
}

#[cfg(not(unix))]
pub(super) fn read(_path: &Path, _maximum: usize) -> Result<Zeroizing<Vec<u8>>, &'static str> {
    Err("operator_file_platform_unsupported")
}
