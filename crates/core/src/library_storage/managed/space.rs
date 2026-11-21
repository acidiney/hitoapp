use super::ManagedFiles;
use crate::{Error, Result};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DiskSpace {
    pub available: u64,
    pub total: u64,
}

impl DiskSpace {
    pub(crate) fn below_percent(&self, percent: u64) -> bool {
        u128::from(self.available) * 100 < u128::from(self.total) * u128::from(percent)
    }
}

impl ManagedFiles {
    pub(crate) fn disk_space(&self) -> Result<DiskSpace> {
        #[cfg(any(test, feature = "test-support"))]
        if let Some(space) = *self.simulated_space.lock().map_err(|_| unreadable())? {
            return Ok(space);
        }
        let root = self.root()?;
        measure(&root.directory)
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn simulate_disk_space(&self, space: Option<DiskSpace>) {
        if let Ok(mut simulated) = self.simulated_space.lock() {
            *simulated = space;
        }
    }
}

#[cfg(unix)]
fn measure(directory: &cap_std::fs::Dir) -> Result<DiskSpace> {
    use std::os::fd::AsRawFd;
    let mut stat = std::mem::MaybeUninit::<libc::statvfs>::uninit();
    if unsafe { libc::fstatvfs(directory.as_raw_fd(), stat.as_mut_ptr()) } != 0 {
        return Err(unreadable());
    }
    let stat = unsafe { stat.assume_init() };
    let block = stat.f_frsize as u128;
    let bytes = |blocks: u128| u64::try_from(blocks * block).unwrap_or(u64::MAX);
    Ok(DiskSpace {
        available: bytes(stat.f_bavail as u128),
        total: bytes(stat.f_blocks as u128),
    })
}

#[cfg(not(unix))]
fn measure(_directory: &cap_std::fs::Dir) -> Result<DiskSpace> {
    Err(unreadable())
}

fn unreadable() -> Error {
    Error::Internal("Free disk space could not be read".into())
}
