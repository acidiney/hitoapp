use crate::{catalog::Catalog, inspection};
use cap_fs_ext::DirExt;
use cap_std::fs::Dir;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    io,
    path::Path,
    sync::Arc,
    time::{Duration, Instant},
};

pub(crate) fn scan(path: &Path) -> io::Result<Catalog> {
    let started = Instant::now();
    let deadline = started + Duration::from_secs(300);
    if std::fs::symlink_metadata(path)?.file_type().is_symlink() {
        return Err(io::Error::other("Folder root must not be a symlink"));
    }
    let root = Arc::new(Dir::open_ambient_dir(path, cap_std::ambient_authority())?);
    let mut pending = vec![(root, 0)];
    let mut tracks = BTreeMap::new();
    let mut visited = 0;
    let mut folders = 0;
    let mut bytes = 0u64;
    let mut skipped = 0;
    while let Some((directory, depth)) = pending.pop() {
        folders += 1;
        if folders > 1024 {
            return Err(io::Error::other("Folder count exceeds scan limit"));
        }
        for entry in directory.entries()? {
            inspection::check(deadline)?;
            visited += 1;
            if visited > 50000 {
                return Err(io::Error::other("Directory entries exceed scan limit"));
            }
            let entry = entry?;
            let kind = entry.file_type()?;
            let name = entry.file_name();
            if kind.is_dir() {
                if depth >= 32 {
                    return Err(io::Error::other("Folder depth exceeds scan limit"));
                }
                if folders + pending.len() >= 1024 {
                    return Err(io::Error::other("Folder count exceeds scan limit"));
                }
                if let Ok(child) = directory.open_dir_nofollow(&name) {
                    pending.push((Arc::new(child), depth + 1));
                }
            } else if kind.is_file() {
                match inspection::inspect(directory.clone(), name, deadline) {
                    Ok(track) => {
                        bytes += track.stamp.size;
                        if bytes > 16 * 1024 * 1024 * 1024 || tracks.len() >= 10000 {
                            return Err(io::Error::other("Media collection exceeds scan limit"));
                        }
                        tracks.entry(track.id.clone()).or_insert(track);
                    }
                    Err(_) => skipped += 1,
                }
            }
        }
    }
    crate::releases::assign(&mut tracks);
    tracing::info!(
        event = "folder_scan_complete",
        recordings = tracks.len(),
        visited,
        skipped,
        elapsed_ms = started.elapsed().as_millis() as u64,
        "Folder catalog ready"
    );
    let mut fingerprint = Sha256::new();
    for id in tracks.keys() {
        fingerprint.update(id.as_bytes());
    }
    Ok(Catalog {
        tracks,
        fingerprint: format!("{:x}", fingerprint.finalize()),
    })
}
