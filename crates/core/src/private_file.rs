//! Ownership rules shared by files that hold credential material.
//!
//! On Unix a private file is a regular file with one hard link, owned by the
//! effective user, whose permission bits are one of the accepted modes.
//! Windows has no verified owner-only ACL check yet, so it never qualifies.

use std::fs::Metadata;

#[cfg(unix)]
pub(crate) fn is_private(metadata: &Metadata, accepted_modes: &[u32]) -> bool {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    metadata.is_file()
        && metadata.nlink() == 1
        && metadata.uid() == unsafe { libc::geteuid() }
        && accepted_modes.contains(&(metadata.permissions().mode() & 0o7777))
}

#[cfg(not(unix))]
pub(crate) fn is_private(_metadata: &Metadata, _accepted_modes: &[u32]) -> bool {
    false
}
