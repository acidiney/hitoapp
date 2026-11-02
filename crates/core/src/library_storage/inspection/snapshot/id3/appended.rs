use super::{Control, MAX_TAG_BYTES, Report, attempt, read_at, write_at};
use crate::Result;
use std::fs::File;

const HEADER: u64 = 10;
const ID3V1_BYTES: u64 = 128;
const COPY_CHUNK: usize = 64 * 1024;

struct Appended {
    start: u64,
    span: u64,
    trailer: u64,
}

pub(super) fn settle(
    file: &mut File,
    length: u64,
    control: &Control,
    report: &mut Report,
) -> Result<u64> {
    let (insert_at, stacked) = leading_tags(file, length, report);
    let Some(appended) =
        locate(file, length, report).filter(|found| found.start >= insert_at.max(1))
    else {
        return Ok(length);
    };
    if stacked >= super::MAX_STACKED_TAGS || appended.span > MAX_TAG_BYTES {
        report.appended_dropped += 1;
        return cut(file, length, &appended, control, report);
    }
    report.appended_moved += 1;
    insert(file, length, insert_at, &appended, control, report)
}

fn leading_tags(file: &mut File, length: u64, report: &mut Report) -> (u64, usize) {
    let mut offset = 0;
    let mut stacked = 0;
    for _ in 0..super::MAX_STACKED_TAGS {
        let mut header = [0u8; 10];
        if attempt(read_at(file, offset, &mut header), report) != Some(10) {
            break;
        }
        match super::rewrite::span(&header) {
            Some(span) if offset + span <= length => {
                offset += span;
                stacked += 1;
            }
            _ => break,
        }
    }
    (offset, stacked)
}

fn locate(file: &mut File, length: u64, report: &mut Report) -> Option<Appended> {
    let mut trailers = vec![0];
    if length > ID3V1_BYTES {
        let mut marker = [0u8; 3];
        if attempt(read_at(file, length - ID3V1_BYTES, &mut marker), report) == Some(3)
            && &marker == b"TAG"
        {
            trailers = vec![ID3V1_BYTES];
        }
    }
    trailers
        .into_iter()
        .find_map(|trailer| footer_at(file, length, trailer, report))
}

fn footer_at(file: &mut File, length: u64, trailer: u64, report: &mut Report) -> Option<Appended> {
    let footer_start = length.checked_sub(trailer + HEADER)?;
    let mut footer = [0u8; 10];
    if attempt(read_at(file, footer_start, &mut footer), report) != Some(10) {
        return None;
    }
    if &footer[..3] != b"3DI" || footer[3] != 4 || footer[5] & 0x10 == 0 {
        return None;
    }
    let size = u64::from(super::rewrite::syncsafe(&footer[6..10])?);
    let span = size + 2 * HEADER;
    let start = (footer_start + HEADER).checked_sub(span)?;
    let mut header = [0u8; 10];
    if attempt(read_at(file, start, &mut header), report) != Some(10) {
        return None;
    }
    let matches_footer = &header[..3] == b"ID3"
        && header[3] == 4
        && header[5] == footer[5]
        && header[6..10] == footer[6..10];
    matches_footer.then_some(Appended {
        start,
        span,
        trailer,
    })
}

fn cut(
    file: &mut File,
    length: u64,
    appended: &Appended,
    control: &Control,
    report: &mut Report,
) -> Result<u64> {
    let mut trailer = vec![0u8; appended.trailer as usize];
    let trailer_start = length - appended.trailer;
    if trailer_start != appended.start + appended.span {
        return Ok(length);
    }
    control.check()?;
    if attempt(read_at(file, trailer_start, &mut trailer), report) != Some(trailer.len()) {
        return Ok(length);
    }
    let rewritten = write_at(file, appended.start, &trailer)
        .and_then(|()| file.set_len(appended.start + appended.trailer));
    match rewritten {
        Ok(()) => Ok(appended.start + appended.trailer),
        Err(_) => Err(unverified()),
    }
}

fn insert(
    file: &mut File,
    length: u64,
    insert_at: u64,
    appended: &Appended,
    control: &Control,
    report: &mut Report,
) -> Result<u64> {
    if appended.start + appended.span + appended.trailer != length {
        return Ok(length);
    }
    let mut tag = vec![0u8; appended.span as usize];
    control.check()?;
    if attempt(read_at(file, appended.start, &mut tag), report) != Some(tag.len()) {
        return Ok(length);
    }
    let mut remaining = appended.start - insert_at;
    let mut buffer = vec![0u8; COPY_CHUNK];
    while remaining > 0 {
        control.check()?;
        let count = remaining.min(COPY_CHUNK as u64) as usize;
        remaining -= count as u64;
        let from = insert_at + remaining;
        let chunk = &mut buffer[..count];
        if read_at(file, from, chunk).ok() != Some(count)
            || write_at(file, from + appended.span, chunk).is_err()
        {
            return Err(unverified());
        }
    }
    write_at(file, insert_at, &tag).map_err(|_| unverified())?;
    Ok(length)
}

fn unverified() -> crate::Error {
    crate::Error::Internal("Media file could not be verified".into())
}
