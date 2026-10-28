use super::{Control, unreadable};
use crate::Result;
use flate2::read::ZlibDecoder;
use std::{
    fs::File,
    io::{Read, Seek, SeekFrom, Write},
};

mod rewrite;

pub(super) use rewrite::Report;

const MAX_TAG_BYTES: u64 = 16 * 1024 * 1024;
const MAX_CHUNKS: usize = 4096;
const MAX_STACKED_TAGS: usize = 4;

#[derive(Clone, Copy)]
enum Endian {
    Little,
    Big,
}

impl Endian {
    fn u32(self, bytes: [u8; 4]) -> u64 {
        u64::from(match self {
            Self::Little => u32::from_le_bytes(bytes),
            Self::Big => u32::from_be_bytes(bytes),
        })
    }
}

/// Prepares ID3v2 tags in the private media copy for FFmpeg. Compressed text
/// frames become plain frames of equal total length, and frames that break the
/// limits are dropped. In RIFF files with an ID3 chunk, the chunk is renamed to
/// `id3 ` and `LIST`/`INFO` chunks to `JUNK`, because FFmpeg ignores the ID3
/// tag when an INFO list exists. The ID3 tag therefore takes precedence.
pub(super) fn neutralise(file: &mut File, control: &Control) -> Result<Report> {
    let length = file.metadata().map_err(|_| unreadable())?.len();
    let mut report = Report::default();
    let mut head = [0u8; 12];
    let read = read_at(file, 0, &mut head)?;
    if read >= 10 && head.starts_with(b"ID3") {
        let mut offset = 0;
        for _ in 0..MAX_STACKED_TAGS {
            control.check()?;
            let Some(span) = tag_at(file, offset, length - offset, &mut report)? else {
                break;
            };
            offset += span;
            let mut next = [0u8; 3];
            if read_at(file, offset, &mut next)? < 3 || &next != b"ID3" {
                break;
            }
        }
    } else if read == 12 && &head[8..12] == b"WAVE" && head.starts_with(b"RIFF") {
        chunk_tags(file, length, Endian::Little, control, &mut report)?;
    } else if read == 12 && head.starts_with(b"FORM") && matches!(&head[8..12], b"AIFF" | b"AIFC") {
        chunk_tags(file, length, Endian::Big, control, &mut report)?;
    }
    Ok(report)
}

fn chunk_tags(
    file: &mut File,
    length: u64,
    endian: Endian,
    control: &Control,
    report: &mut Report,
) -> Result<()> {
    let mut offset = 12;
    let mut info_lists = Vec::new();
    let mut id3_chunks = Vec::new();
    for _ in 0..MAX_CHUNKS {
        control.check()?;
        let mut header = [0u8; 8];
        if offset + 8 > length || read_at(file, offset, &mut header)? < 8 {
            break;
        }
        let size = endian.u32([header[4], header[5], header[6], header[7]]);
        let body = offset + 8;
        if size > length - body {
            break;
        }
        match &header[..4] {
            b"id3 " | b"ID3 " => {
                if tag_at(file, body, size, report)?.is_some() {
                    id3_chunks.push(offset);
                }
            }
            b"LIST" => {
                let mut form = [0u8; 4];
                if read_at(file, body, &mut form)? == 4 && &form == b"INFO" {
                    info_lists.push(offset);
                }
            }
            _ => {}
        }
        offset = body + size + (size & 1);
    }
    if matches!(endian, Endian::Little) && !id3_chunks.is_empty() {
        for (offset, name) in id3_chunks
            .into_iter()
            .map(|offset| (offset, b"id3 "))
            .chain(info_lists.into_iter().map(|offset| (offset, b"JUNK")))
        {
            write_at(file, offset, name)?;
        }
    }
    Ok(())
}

fn tag_at(
    file: &mut File,
    offset: u64,
    available: u64,
    report: &mut Report,
) -> Result<Option<u64>> {
    let mut header = [0u8; 10];
    if available < 10 || read_at(file, offset, &mut header)? < 10 {
        return Ok(None);
    }
    let Some(span) = rewrite::span(&header) else {
        return Ok(None);
    };
    if span > available || span > MAX_TAG_BYTES {
        return Ok(None);
    }
    let mut tag = vec![0u8; span as usize];
    if read_at(file, offset, &mut tag)? < tag.len() {
        return Ok(None);
    }
    if let Some(replacement) = rewrite::normalise(&tag, report) {
        debug_assert_eq!(replacement.len(), tag.len());
        file.seek(SeekFrom::Start(offset))
            .map_err(|_| unreadable())?;
        file.write_all(&replacement).map_err(|_| unreadable())?;
    }
    Ok(Some(span))
}

fn write_at(file: &mut File, offset: u64, bytes: &[u8; 4]) -> Result<()> {
    file.seek(SeekFrom::Start(offset))
        .map_err(|_| unreadable())?;
    file.write_all(bytes).map_err(|_| unreadable())
}

fn read_at(file: &mut File, offset: u64, buffer: &mut [u8]) -> Result<usize> {
    file.seek(SeekFrom::Start(offset))
        .map_err(|_| unreadable())?;
    let mut filled = 0;
    while filled < buffer.len() {
        match file.read(&mut buffer[filled..]) {
            Ok(0) => break,
            Ok(count) => filled += count,
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
            Err(_) => return Err(unreadable()),
        }
    }
    Ok(filled)
}

fn inflate(compressed: &[u8], declared: usize) -> Option<Vec<u8>> {
    let mut output = Vec::with_capacity(declared);
    ZlibDecoder::new(compressed)
        .take(declared as u64 + 1)
        .read_to_end(&mut output)
        .ok()?;
    (output.len() == declared).then_some(output)
}
