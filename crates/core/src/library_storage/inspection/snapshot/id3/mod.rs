use super::Control;
use crate::Result;
use std::{
    fs::File,
    io::{self, Read, Seek, SeekFrom, Write},
};

mod appended;
mod rewrite;
#[cfg(test)]
mod tests;

use crate::id3_frames::tag_span;
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

pub(super) fn neutralise(file: &mut File, control: &Control) -> Result<Report> {
    let mut report = Report::default();
    scan(file, control, &mut report)?;
    Ok(report)
}

fn scan(file: &mut File, control: &Control, report: &mut Report) -> Result<()> {
    let Some(length) = attempt(file.metadata().map(|m| m.len()), report) else {
        return Ok(());
    };
    let length = appended::settle(file, length, control, report)?;
    let mut head = [0u8; 12];
    let Some(read) = attempt(read_at(file, 0, &mut head), report) else {
        return Ok(());
    };
    if read >= 10 && head.starts_with(b"ID3") {
        let mut offset = 0;
        for _ in 0..MAX_STACKED_TAGS {
            control.check()?;
            let Some(span) = tag_at(file, offset, length - offset, control, report)? else {
                break;
            };
            offset += span;
            let mut next = [0u8; 3];
            match attempt(read_at(file, offset, &mut next), report) {
                Some(3) if &next == b"ID3" => {}
                _ => break,
            }
        }
    } else if read == 12 && &head[8..12] == b"WAVE" && head.starts_with(b"RIFF") {
        chunk_tags(file, length, Endian::Little, control, report)?;
    } else if read == 12 && head.starts_with(b"FORM") && matches!(&head[8..12], b"AIFF" | b"AIFC") {
        chunk_tags(file, length, Endian::Big, control, report)?;
    }
    Ok(())
}

fn attempt<T>(result: io::Result<T>, report: &mut Report) -> Option<T> {
    match result {
        Ok(value) => Some(value),
        Err(_) => {
            report.io_failed += 1;
            None
        }
    }
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
        if offset + 8 > length {
            break;
        }
        match attempt(read_at(file, offset, &mut header), report) {
            Some(8) => {}
            _ => break,
        }
        let size = endian.u32([header[4], header[5], header[6], header[7]]);
        let body = offset + 8;
        if size > length - body {
            break;
        }
        match &header[..4] {
            b"id3 " | b"ID3 " => {
                if tag_at(file, body, size, control, report)?.is_some() {
                    id3_chunks.push(offset);
                }
            }
            b"LIST" => {
                let mut form = [0u8; 4];
                if matches!(attempt(read_at(file, body, &mut form), report), Some(4))
                    && &form == b"INFO"
                {
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
            attempt(write_at(file, offset, name), report);
        }
    }
    Ok(())
}

fn tag_at(
    file: &mut File,
    offset: u64,
    available: u64,
    control: &Control,
    report: &mut Report,
) -> Result<Option<u64>> {
    let mut header = [0u8; 10];
    if available < 10 {
        return Ok(None);
    }
    match attempt(read_at(file, offset, &mut header), report) {
        Some(10) => {}
        _ => return Ok(None),
    }
    let Some(span) = tag_span(&header) else {
        return Ok(None);
    };
    if span > available || span > MAX_TAG_BYTES {
        return Ok(None);
    }
    let mut tag = vec![0u8; span as usize];
    match attempt(read_at(file, offset, &mut tag), report) {
        Some(read) if read == tag.len() => {}
        _ => return Ok(None),
    }
    if let Some(replacement) = rewrite::normalise(&tag, report, control)? {
        debug_assert_eq!(replacement.len(), tag.len());
        if attempt(write_at(file, offset, &replacement), report).is_none()
            && attempt(write_at(file, offset, &tag), report).is_none()
        {
            let mut current = vec![0u8; tag.len()];
            let intact = read_at(file, offset, &mut current).is_ok_and(|read| read == tag.len())
                && current == tag;
            if !intact {
                return Err(crate::Error::Internal(
                    "Media file could not be verified".into(),
                ));
            }
        }
    }
    Ok(Some(span))
}

fn write_at(file: &mut File, offset: u64, bytes: &[u8]) -> io::Result<()> {
    file.seek(SeekFrom::Start(offset))?;
    file.write_all(bytes)
}

fn read_at(file: &mut File, offset: u64, buffer: &mut [u8]) -> io::Result<usize> {
    file.seek(SeekFrom::Start(offset))?;
    let mut filled = 0;
    while filled < buffer.len() {
        match file.read(&mut buffer[filled..]) {
            Ok(0) => break,
            Ok(count) => filled += count,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => return Err(error),
        }
    }
    Ok(filled)
}
