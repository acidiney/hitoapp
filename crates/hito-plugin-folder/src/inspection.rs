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
    io::{self, BufReader, Read, Seek, SeekFrom},
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
    if stamp.size == 0 || stamp.size > 1024 * 1024 * 1024 {
        return Err(InspectionError::Unavailable);
    }
    let mut reader = Budget {
        file,
        remaining: 8 * 1024 * 1024,
        deadline,
        exhausted: false,
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
    let mut hasher = Sha256::new();
    let mut remaining = stamp.size;
    let mut buffer = [0; 65536];
    while remaining > 0 {
        check(deadline)?;
        let wanted = remaining.min(buffer.len() as u64) as usize;
        let count = reader.file.read(&mut buffer[..wanted])?;
        if count == 0 {
            return Err(InspectionError::Unavailable);
        }
        hasher.update(&buffer[..count]);
        remaining -= count as u64;
    }
    if Stamp::read(&reader.file.metadata()?)? != stamp {
        return Err(InspectionError::Unavailable);
    }
    let id = format!("recording-{:x}", hasher.finalize());
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
struct Budget {
    file: File,
    remaining: usize,
    deadline: Instant,
    exhausted: bool,
}
impl Read for Budget {
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
        let count = self.file.read(&mut buffer[..maximum])?;
        self.remaining -= count;
        Ok(count)
    }
}
impl Seek for Budget {
    fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
        check(self.deadline)?;
        self.file.seek(position)
    }
}

#[derive(Debug)]
pub(crate) enum InspectionError {
    Unavailable,
    Deadline,
    MetadataBudget,
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
        }
    }
}
