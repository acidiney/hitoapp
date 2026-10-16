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
    scan_with_inspector(path, inspection::inspect)
}
pub(crate) fn scan_owned(path: &Path, claim: Arc<crate::registry::Claim>) -> io::Result<Catalog> {
    let mut work = inspection::WorkBudget::new();
    scan_with_inspector(path, |directory, name, deadline| {
        inspection::inspect_snapshot(directory, name, deadline, &claim, &mut work)
    })
}
fn scan_with_inspector(
    path: &Path,
    mut inspect: impl FnMut(
        Arc<Dir>,
        std::ffi::OsString,
        Instant,
    ) -> Result<crate::catalog::Track, inspection::InspectionError>,
) -> io::Result<Catalog> {
    let started = Instant::now();
    let deadline = started + Duration::from_secs(300);
    let root = Arc::new(open_root(path)?);
    let mut pending = vec![(root, 0, std::path::PathBuf::new())];
    let mut observations = Vec::new();
    let mut visited = 0;
    let mut folders = 0;
    let mut bytes = 0u64;
    let mut skipped = 0;
    let mut retained_bytes = 0usize;
    while let Some((directory, depth, relative)) = pending.pop() {
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
                    pending.push((Arc::new(child), depth + 1, relative.join(&name)));
                }
            } else if kind.is_file() {
                let locator = relative.join(&name).as_os_str().as_encoded_bytes().to_vec();
                if locator.len() > 16384 {
                    return Err(io::Error::other("Folder locator exceeds scan limit"));
                }
                match inspect(directory.clone(), name, deadline) {
                    Ok(mut track) => {
                        track.locator = locator;
                        bytes += track.stamp.size;
                        retained_bytes += observation_bytes(&track);
                        if bytes > 16 * 1024 * 1024 * 1024
                            || observations.len() >= 10000
                            || retained_bytes > 32 * 1024 * 1024
                        {
                            return Err(io::Error::other("Media collection exceeds scan limit"));
                        }
                        observations.push(track);
                    }
                    Err(inspection::InspectionError::Unavailable) => skipped += 1,
                    Err(error) => {
                        tracing::debug!(event = "folder_scan_failed", outcome = error.code());
                        return Err(error.into());
                    }
                }
            }
        }
    }
    inspection::check(deadline)?;
    crate::releases::assign(&mut observations);
    let mut tracks = BTreeMap::new();
    for track in &observations {
        tracks
            .entry(track.id.clone())
            .or_insert_with(|| track.clone());
    }
    let mut fingerprint = Sha256::new();
    for id in tracks.keys() {
        fingerprint.update(id.as_bytes());
    }
    inspection::check(deadline)?;
    tracing::info!(
        event = "folder_scan_complete",
        recordings = tracks.len(),
        observations = observations.len(),
        visited,
        skipped,
        elapsed_ms = started.elapsed().as_millis() as u64,
        "Folder catalog ready"
    );
    let mut catalog = Catalog {
        sources: BTreeMap::new(),
        recording_sources: BTreeMap::new(),
        retained: tracks
            .iter()
            .map(|(id, track)| (id.clone(), track.recording()))
            .collect(),
        tracks,
        observations,
        fingerprint: format!("{:x}", fingerprint.finalize()),
        claim: None,
    };
    catalog.index_sources()?;
    Ok(catalog)
}

fn observation_bytes(track: &crate::catalog::Track) -> usize {
    [
        &track.id,
        &track.checksum,
        &track.source,
        &track.revision,
        &track.title,
        &track.searchable,
        &track.artist,
        &track.album_artist,
    ]
    .into_iter()
    .map(String::len)
    .sum::<usize>()
        + track.album.as_ref().map_or(0, String::len)
        + track.evidence.as_ref().map_or(0, String::len)
        + track.locator.len()
        + track.name.as_encoded_bytes().len()
        + 512
}

pub(crate) fn open_root(path: &Path) -> io::Result<Dir> {
    let absolute = std::path::absolute(path)?;
    let Some(parent) = absolute.parent() else {
        return Dir::open_ambient_dir(absolute, cap_std::ambient_authority());
    };
    let name = absolute.file_name().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "Select a folder without a trailing parent component",
        )
    })?;
    let parent = Dir::open_ambient_dir(parent, cap_std::ambient_authority())?;
    parent.open_dir_nofollow(name)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[allow(dead_code)]
    mod fixture {
        include!(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/support/mod.rs"));
    }
    #[test]
    fn deadline_failure_during_the_final_inspection_cannot_publish_a_partial_catalog() {
        let directory = tempfile::tempdir().unwrap();
        fixture::write_wav(
            &directory.path().join("last.wav"),
            Some("Deadline fixture"),
            440,
        );
        let mut inspected = 0;
        let result = scan_with_inspector(directory.path(), |directory, name, deadline| {
            let track = inspection::inspect(directory, name, deadline)?;
            assert_eq!(track.title, "Deadline fixture");
            inspected += 1;
            Err(inspection::InspectionError::Deadline)
        });
        assert_eq!(inspected, 1);
        assert_eq!(
            result
                .err()
                .expect("Final inspection timeout must fail the scan")
                .kind(),
            io::ErrorKind::TimedOut
        );
    }
    #[test]
    fn metadata_budget_failure_cannot_publish_a_partial_catalog() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("oversized.wav");
        let mut bytes = fixture::write_wav(&path, None, 440);
        let mut info = b"INFOINAM".to_vec();
        let size = 9 * 1024 * 1024u32;
        info.extend(size.to_le_bytes());
        info.resize(info.len() + size as usize, b'x');
        bytes.extend(b"LIST");
        bytes.extend((info.len() as u32).to_le_bytes());
        bytes.extend(info);
        let size = (bytes.len() - 8) as u32;
        bytes[4..8].copy_from_slice(&size.to_le_bytes());
        std::fs::write(&path, bytes).unwrap();
        let result = crate::Folder::scan(directory.path());
        assert_eq!(
            result
                .err()
                .expect("Metadata budget exhaustion must fail the scan")
                .kind(),
            io::ErrorKind::InvalidData
        );
    }
}
