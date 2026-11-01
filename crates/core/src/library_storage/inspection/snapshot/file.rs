use super::{Control, MediaSnapshot, Mode, Prepared, Snapshot, changed, unreadable};
use crate::{Result, library_storage::root::Root};
use sha2::{Digest, Sha256};
use std::{
    fs::{File, Metadata, OpenOptions},
    io::{Read, Seek, Write},
    path::Path,
    sync::Arc,
    time::SystemTime,
};

#[derive(PartialEq)]
struct Stamp {
    size: u64,
    modified: SystemTime,
    #[cfg(unix)]
    identity: (u64, u64, i64, i64),
    #[cfg(windows)]
    created: u64,
}

impl Stamp {
    fn read(metadata: Metadata) -> Result<Self> {
        if !metadata.is_file() {
            return Err(unreadable());
        }
        Ok(Self {
            size: metadata.len(),
            modified: metadata.modified().map_err(|_| unreadable())?,
            #[cfg(unix)]
            identity: {
                use std::os::unix::fs::MetadataExt;
                (
                    metadata.dev(),
                    metadata.ino(),
                    metadata.ctime(),
                    metadata.ctime_nsec(),
                )
            },
            #[cfg(windows)]
            created: {
                use std::os::windows::fs::MetadataExt;
                metadata.creation_time()
            },
        })
    }
}

pub(super) fn read(root: &Root, path: &Path, mode: Mode, control: &Control) -> Result<Prepared> {
    control.check()?;
    let path = path.to_owned();
    let mut file = root.open_file(&path).map_err(|_| unreadable())?;
    let initial = Stamp::read(file.metadata().map_err(|_| unreadable())?)?;
    let size = i64::try_from(initial.size).map_err(|_| unreadable())?;
    let (checksum_only, cached) = match mode {
        Mode::Checksum => (true, None),
        Mode::Prepare(cached) => (false, cached),
    };
    if checksum_only || cached.is_some() {
        let checksum = transfer(&mut file, initial.size, None, control)?;
        verify_unchanged(root, &path, &file, &initial)?;
        control.check()?;
        if checksum_only || cached.as_deref() == Some(checksum.as_str()) {
            return Ok(Prepared {
                file: Snapshot {
                    path,
                    checksum,
                    size,
                },
                media: None,
            });
        }
        file.rewind().map_err(|_| unreadable())?;
    }
    control.check()?;
    let mut directory = tempfile::Builder::new();
    directory.prefix("hito-media-");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        directory.permissions(std::fs::Permissions::from_mode(0o700));
    }
    let directory = Arc::new(directory.tempdir().map_err(|_| unreadable())?);
    let media_path = directory.path().join("media");
    let mut output = OpenOptions::new();
    output.read(true).write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        output.mode(0o600).custom_flags(libc::O_NOFOLLOW);
    }
    let mut output = output.open(&media_path).map_err(|_| unreadable())?;
    let checksum = transfer(&mut file, initial.size, Some(&mut output), control)?;
    output.flush().map_err(|_| unreadable())?;
    let tags = super::id3::neutralise(&mut output, control)?;
    drop(output);
    let artists = super::super::tagged_artists::read(&media_path);
    if tags.decompressed > 0 || tags.ignored() > 0 || tags.io_failed > 0 {
        tracing::debug!(
            event = "id3_compressed_frames",
            decompressed = tags.decompressed,
            ignored_over_limit = tags.ignored_over_limit,
            ignored_invalid = tags.ignored_invalid,
            ignored_unsupported = tags.ignored_unsupported,
            io_failed = tags.io_failed,
            "Compressed ID3 frames normalised in the private media copy"
        );
    }
    verify_unchanged(root, &path, &file, &initial)?;
    control.check()?;
    Ok(Prepared {
        file: Snapshot {
            path,
            checksum,
            size,
        },
        media: Some(MediaSnapshot {
            path: media_path,
            directory,
            recording_id: tags.recording_id,
            artists,
        }),
    })
}

fn transfer(
    input: &mut File,
    original_size: u64,
    mut output: Option<&mut File>,
    control: &Control,
) -> Result<String> {
    let mut remaining = original_size;
    let mut hasher = Sha256::new();
    let mut buffer = [0; 65536];
    while remaining > 0 {
        control.check()?;
        let wanted = remaining.min(buffer.len() as u64) as usize;
        let count = input
            .read(&mut buffer[..wanted])
            .map_err(|_| unreadable())?;
        if count == 0 {
            return Err(changed());
        }
        control.check()?;
        if let Some(output) = output.as_deref_mut() {
            output
                .write_all(&buffer[..count])
                .map_err(|_| unreadable())?;
        }
        hasher.update(&buffer[..count]);
        remaining -= count as u64;
    }
    control.check()?;
    let mut extra = [0];
    if input.read(&mut extra).map_err(|_| unreadable())? != 0 {
        return Err(changed());
    }
    control.check()?;
    Ok(format!("{:x}", hasher.finalize()))
}

fn verify_unchanged(root: &Root, path: &Path, file: &File, initial: &Stamp) -> Result<()> {
    let opened = Stamp::read(file.metadata().map_err(|_| unreadable())?)?;
    let located = Stamp::read(
        root.open_file(path)
            .map_err(|_| unreadable())?
            .metadata()
            .map_err(|_| unreadable())?,
    )?;
    if &opened != initial || &located != initial {
        return Err(changed());
    }
    Ok(())
}
