use crate::catalog::{Stamp, Track};
use lofty::{
    config::ParseOptions,
    file::{AudioFile, FileType, TaggedFileExt},
    prelude::Accessor,
    probe::Probe,
};
use sha2::{Digest, Sha256};
use std::{
    fs::File,
    io::{self, BufReader, Read, Seek, SeekFrom, Write},
    sync::Arc,
    time::Instant,
};

pub(crate) fn inspect(
    directory: Arc<cap_std::fs::Dir>,
    name: std::ffi::OsString,
    deadline: Instant,
) -> Result<Track, InspectionError> {
    check(deadline)?;
    let file = crate::catalog::open(&directory, &name)?;
    let stamp = Stamp::read(&file.metadata()?)?;
    inspect_file(
        directory,
        name,
        file,
        stamp,
        deadline,
        &mut WorkBudget::new(),
    )
}

pub(crate) fn inspect_snapshot(
    directory: Arc<cap_std::fs::Dir>,
    name: std::ffi::OsString,
    deadline: Instant,
    claim: &crate::registry::Claim,
    work: &mut WorkBudget,
) -> Result<Track, InspectionError> {
    check(deadline)?;
    let mut original = crate::catalog::open(&directory, &name)?;
    let stamp = Stamp::read(&original.metadata()?)?;
    if stamp.size == 0 || stamp.size > 1024 * 1024 * 1024 {
        return Err(InspectionError::Unavailable);
    }
    let mut snapshot = claim.snapshot().map_err(|error| {
        tracing::debug!(event = "folder_input_failed", stage = "snapshot_create", error_kind = ?error.kind());
        InspectionError::Snapshot
    })?;
    let mut remaining = stamp.size;
    let mut buffer = [0; 65536];
    while remaining > 0 {
        check(deadline)?;
        let wanted = remaining.min(buffer.len() as u64) as usize;
        let count = work.read(&mut original, &mut buffer[..wanted])?;
        if count == 0 {
            return Err(InspectionError::Changed);
        }
        snapshot
            .write_all(&buffer[..count])
            .map_err(|_| InspectionError::Snapshot)?;
        remaining -= count as u64;
    }
    if Stamp::read(&original.metadata()?)? != stamp {
        return Err(InspectionError::Changed);
    }
    snapshot.rewind().map_err(|_| InspectionError::Snapshot)?;
    let evidence = crate::pcm::evidence(&mut snapshot, stamp.size, deadline, work)?;
    snapshot.rewind().map_err(|_| InspectionError::Snapshot)?;
    let mut track = inspect_file(directory, name, snapshot, stamp, deadline, work)?;
    track.evidence = evidence;
    let mut current = crate::catalog::open(&track.directory, &track.name)
        .map_err(|_| InspectionError::Changed)?;
    if Stamp::read(&current.metadata()?)? != track.stamp {
        return Err(InspectionError::Changed);
    }
    let checksum = checksum(&mut current, track.stamp.size, deadline, work)?;
    if checksum != track.checksum || Stamp::read(&current.metadata()?)? != track.stamp {
        return Err(InspectionError::Changed);
    }
    Ok(track)
}

fn inspect_file(
    directory: Arc<cap_std::fs::Dir>,
    name: std::ffi::OsString,
    file: File,
    stamp: Stamp,
    deadline: Instant,
    work: &mut WorkBudget,
) -> Result<Track, InspectionError> {
    if stamp.size == 0 || stamp.size > 1024 * 1024 * 1024 {
        return Err(InspectionError::Unavailable);
    }
    let input_stamp = Stamp::read(&file.metadata()?)?;
    let mut reader = Budget {
        file,
        remaining: 8 * 1024 * 1024,
        deadline,
        exhausted: false,
        work,
    };
    let result = Probe::new(BufReader::new(&mut reader))
        .options(ParseOptions::new().read_cover_art(false))
        .guess_file_type()
        .and_then(|probe| {
            probe
                .read()
                .map_err(|_| io::Error::other("Unsupported media"))
        });
    check(deadline)?;
    if reader.work.exhausted {
        return Err(InspectionError::WorkBudget);
    }
    if reader.work.failed {
        return Err(InspectionError::Snapshot);
    }
    if reader.exhausted {
        return Err(InspectionError::MetadataBudget);
    }
    let tagged = result.map_err(|_| InspectionError::Unavailable)?;
    let mime = match tagged.file_type() {
        FileType::Wav => "audio/wav",
        FileType::Flac => "audio/flac",
        FileType::Mpeg => "audio/mpeg",
        FileType::Mp4 => "audio/mp4",
        FileType::Aac => "audio/aac",
        FileType::Vorbis | FileType::Opus => "audio/ogg",
        FileType::Aiff => "audio/aiff",
        _ => return Err(InspectionError::Unavailable),
    };
    reader.file.rewind()?;
    let checksum = checksum(&mut reader.file, stamp.size, deadline, reader.work)?;
    if Stamp::read(&reader.file.metadata()?)? != input_stamp {
        return Err(InspectionError::Changed);
    }
    let id = format!("recording-{checksum}");
    let tag = tagged.primary_tag().or_else(|| tagged.first_tag());
    let title = tag
        .and_then(|tag| text(tag.title().as_deref()))
        .unwrap_or_else(|| format!("Untagged audio {}", &id[10..22]));
    let artist = tag
        .and_then(|tag| text(tag.artist().as_deref()))
        .unwrap_or_else(|| "Unknown artist".into());
    let album = tag.and_then(|tag| text(tag.album().as_deref()));
    let track_number = tag.and_then(Accessor::track).filter(|number| *number > 0);
    let disc_number = tag.and_then(Accessor::disk).filter(|number| *number > 0);
    let album_artist = tag
        .and_then(|tag| text(tag.get_string(&lofty::tag::ItemKey::AlbumArtist)))
        .unwrap_or_else(|| artist.clone());
    let duration_ms = tagged
        .properties()
        .duration()
        .as_millis()
        .min(9_007_199_254_740_991) as u64;
    let searchable =
        format!("{title}\n{artist}\n{}", album.as_deref().unwrap_or("")).to_lowercase();
    check(deadline)?;
    Ok(Track {
        searchable,
        source: format!("source-{id}"),
        revision: id.clone(),
        checksum,
        evidence: None,
        id,
        title,
        artist,
        album,
        release_id: None,
        album_artist,
        duration_ms,
        track_number,
        disc_number,
        mime,
        directory,
        name,
        stamp,
    })
}
fn text(value: Option<&str>) -> Option<String> {
    let value = value?.trim();
    if value.is_empty() || value.len() > 2048 || value.chars().any(char::is_control) {
        return None;
    }
    Some(value.into())
}
pub(crate) fn check(deadline: Instant) -> io::Result<()> {
    if Instant::now() > deadline {
        Err(io::Error::new(
            io::ErrorKind::TimedOut,
            "Folder scan deadline exceeded",
        ))
    } else {
        Ok(())
    }
}
struct Budget<'a> {
    file: File,
    remaining: usize,
    deadline: Instant,
    exhausted: bool,
    work: &'a mut WorkBudget,
}
impl Read for Budget<'_> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        check(self.deadline)?;
        if buffer.is_empty() {
            return Ok(0);
        }
        if self.remaining == 0 {
            self.exhausted = true;
            return Err(io::Error::other("Metadata budget exceeded"));
        }
        let maximum = buffer.len().min(self.remaining);
        let count = self
            .work
            .read(&mut self.file, &mut buffer[..maximum])
            .map_err(io::Error::from)?;
        self.remaining -= count;
        Ok(count)
    }
}
impl Seek for Budget<'_> {
    fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
        check(self.deadline)?;
        self.file.seek(position)
    }
}

pub(crate) struct WorkBudget {
    remaining: u64,
    exhausted: bool,
    failed: bool,
}
impl WorkBudget {
    pub fn new() -> Self {
        Self {
            remaining: 16 * 1024 * 1024 * 1024,
            exhausted: false,
            failed: false,
        }
    }
    pub fn read(&mut self, file: &mut File, bytes: &mut [u8]) -> Result<usize, InspectionError> {
        if bytes.is_empty() {
            return Ok(0);
        }
        if self.remaining == 0 {
            self.exhausted = true;
            return Err(InspectionError::WorkBudget);
        }
        let wanted = self.remaining.min(bytes.len() as u64) as usize;
        let count = file.read(&mut bytes[..wanted]).map_err(|_| {
            self.failed = true;
            InspectionError::Snapshot
        })?;
        self.remaining -= count as u64;
        Ok(count)
    }
    pub fn read_exact(
        &mut self,
        file: &mut File,
        mut bytes: &mut [u8],
        deadline: Instant,
    ) -> Result<(), InspectionError> {
        while !bytes.is_empty() {
            check(deadline)?;
            let count = self.read(file, bytes)?;
            if count == 0 {
                return Err(InspectionError::Changed);
            }
            bytes = &mut bytes[count..];
        }
        Ok(())
    }
}

fn checksum(
    file: &mut File,
    size: u64,
    deadline: Instant,
    work: &mut WorkBudget,
) -> Result<String, InspectionError> {
    let mut hash = Sha256::new();
    let mut remaining = size;
    let mut buffer = [0; 65536];
    while remaining > 0 {
        check(deadline)?;
        let wanted = remaining.min(buffer.len() as u64) as usize;
        let count = work.read(file, &mut buffer[..wanted])?;
        if count == 0 {
            return Err(InspectionError::Changed);
        }
        hash.update(&buffer[..count]);
        remaining -= count as u64;
    }
    Ok(format!("{:x}", hash.finalize()))
}

#[derive(Debug)]
pub(crate) enum InspectionError {
    Unavailable,
    Deadline,
    MetadataBudget,
    WorkBudget,
    Snapshot,
    Changed,
}
impl InspectionError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::Unavailable => "unavailable",
            Self::Deadline => "deadline",
            Self::MetadataBudget => "metadata_budget",
            Self::WorkBudget => "work_budget",
            Self::Snapshot => "input_failed",
            Self::Changed => "changed",
        }
    }
}
impl From<io::Error> for InspectionError {
    fn from(error: io::Error) -> Self {
        if error.kind() == io::ErrorKind::TimedOut {
            Self::Deadline
        } else {
            Self::Unavailable
        }
    }
}
impl From<InspectionError> for io::Error {
    fn from(error: InspectionError) -> Self {
        match error {
            InspectionError::Unavailable => Self::other("Media is unavailable"),
            InspectionError::Deadline => {
                Self::new(io::ErrorKind::TimedOut, "Folder scan deadline exceeded")
            }
            InspectionError::MetadataBudget => {
                Self::new(io::ErrorKind::InvalidData, "Metadata byte budget exceeded")
            }
            InspectionError::WorkBudget => Self::other("Folder inspection read budget exceeded"),
            InspectionError::Snapshot => Self::other("Folder inspection input failed"),
            InspectionError::Changed => Self::other("Folder media changed during inspection"),
        }
    }
}
