use crate::id3_frames::{
    FRAME_HEADER, Frame, MAX_STACKED_TAGS, Tag, Version, deunsynchronise, inflate, syncsafe,
    tag_span,
};
use std::{
    borrow::Cow,
    fs::File,
    io::{Read, Seek, SeekFrom},
};

const MAX_PICTURE_OVERHEAD: usize = 64 * 1024;
const FRONT_COVER: u8 = 3;

pub(super) struct Picture {
    pub front: bool,
    pub bytes: Vec<u8>,
}

pub(super) struct Limits {
    pub tag_bytes: u64,
    pub image_bytes: usize,
}

pub(super) fn read(mut file: File, limits: &Limits) -> Vec<Picture> {
    let mut pictures = Vec::new();
    let mut offset = 0;
    let mut tag_budget = limits.tag_bytes;
    let mut inflate_budget = limits.tag_bytes as usize;
    for _ in 0..MAX_STACKED_TAGS {
        let mut header = [0u8; 10];
        if file.seek(SeekFrom::Start(offset)).is_err() || file.read_exact(&mut header).is_err() {
            break;
        }
        let Some(span) = tag_span(&header).filter(|span| *span <= tag_budget) else {
            break;
        };
        let mut tag = vec![0u8; span as usize];
        if file.seek(SeekFrom::Start(offset)).is_err() || file.read_exact(&mut tag).is_err() {
            break;
        }
        tag_budget -= span;
        offset += span;
        let Some(parsed) = Tag::parse(&tag) else {
            continue;
        };
        for frame in parsed.frames().iter().filter(|frame| &frame.id == b"APIC") {
            let Some(data) = frame_data(parsed.version, frame, limits, &mut inflate_budget) else {
                continue;
            };
            if let Some(picture) = picture(&data) {
                pictures.push(picture);
            }
        }
    }
    pictures
}

fn frame_data<'a>(
    version: Version,
    frame: &'a Frame,
    limits: &Limits,
    budget: &mut usize,
) -> Option<Cow<'a, [u8]>> {
    let flags = frame.flags[1];
    let (compressed, encrypted, grouped, indicated, unsynchronised) = match version {
        Version::V23 => (
            flags & 0x80 != 0,
            flags & 0x40 != 0,
            flags & 0x20 != 0,
            false,
            false,
        ),
        Version::V24 => (
            flags & 0x08 != 0,
            flags & 0x04 != 0,
            flags & 0x40 != 0,
            flags & 0x01 != 0,
            flags & 0x02 != 0,
        ),
    };
    if encrypted {
        return None;
    }
    let mut body: &[u8] = &frame.body;
    let mut declared = None;
    match version {
        Version::V23 => {
            if compressed {
                let (prefix, rest) = body.split_at_checked(4)?;
                declared = Some(u32::from_be_bytes(prefix.try_into().ok()?) as usize);
                body = rest;
            }
            if grouped {
                body = body.get(1..)?;
            }
        }
        Version::V24 => {
            if grouped {
                body = body.get(1..)?;
            }
            if indicated {
                let (prefix, rest) = body.split_at_checked(4)?;
                declared = Some(syncsafe(prefix)? as usize);
                body = rest;
            }
            if compressed && declared.is_none() {
                return None;
            }
        }
    }
    let payload = if unsynchronised {
        Cow::Owned(deunsynchronise(body))
    } else {
        Cow::Borrowed(body)
    };
    if !compressed {
        return Some(match payload {
            Cow::Borrowed(bytes) => Cow::Owned(bytes.to_vec()),
            owned => owned,
        });
    }
    let declared = declared.filter(|declared| *declared > 0)?;
    let cap = limits.image_bytes + MAX_PICTURE_OVERHEAD;
    if declared > cap || payload.len() > cap || declared > *budget {
        return None;
    }
    *budget -= declared;
    inflate(&payload, declared).map(Cow::Owned)
}

fn picture(data: &[u8]) -> Option<Picture> {
    let (&encoding, rest) = data.split_first()?;
    if encoding > 3 {
        return None;
    }
    let mime_end = rest.iter().position(|byte| *byte == 0)?;
    let (&kind, rest) = rest[mime_end + 1..].split_first()?;
    let description_end = if encoding == 1 || encoding == 2 {
        rest.as_chunks::<2>()
            .0
            .iter()
            .position(|pair| pair == &[0, 0])
            .map(|index| index * 2 + 2)?
    } else {
        rest.iter().position(|byte| *byte == 0)? + 1
    };
    Some(Picture {
        front: kind == FRONT_COVER,
        bytes: rest[description_end..].to_vec(),
    })
}

pub(super) fn holds_compressed_picture(file: &mut File, limits: &Limits) -> bool {
    let mut offset = 0;
    let mut tag_budget = limits.tag_bytes;
    for _ in 0..MAX_STACKED_TAGS {
        let mut header = [0u8; 10];
        if file.seek(SeekFrom::Start(offset)).is_err() || file.read_exact(&mut header).is_err() {
            return false;
        }
        let Some(span) = tag_span(&header).filter(|span| *span <= tag_budget) else {
            return false;
        };
        if header[5] & 0xc0 != 0 {
            return true;
        }
        let version = header[3];
        let footer = if version == 4 && header[5] & 0x10 != 0 {
            10
        } else {
            0
        };
        let end = offset + span - footer;
        let mut at = offset + 10;
        while at + FRAME_HEADER as u64 <= end {
            let mut frame = [0u8; 10];
            if file.seek(SeekFrom::Start(at)).is_err() || file.read_exact(&mut frame).is_err() {
                return false;
            }
            if !frame[..4]
                .iter()
                .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit())
            {
                break;
            }
            let compressed_flag = if version == 3 { 0x80 } else { 0x08 };
            if &frame[..4] == b"APIC" && frame[9] & compressed_flag != 0 {
                return true;
            }
            let size = if version == 3 {
                Some(u64::from(u32::from_be_bytes([
                    frame[4], frame[5], frame[6], frame[7],
                ])))
            } else {
                syncsafe(&frame[4..8]).map(u64::from)
            };
            let Some(size) = size else {
                break;
            };
            at += FRAME_HEADER as u64 + size;
        }
        tag_budget -= span;
        offset += span;
    }
    false
}
