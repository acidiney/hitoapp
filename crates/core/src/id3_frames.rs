use flate2::read::ZlibDecoder;
use std::{borrow::Cow, io::Read};

pub(crate) const MAX_FRAMES: usize = 8192;
pub(crate) const FRAME_HEADER: usize = 10;

#[derive(Clone, Copy, PartialEq)]
pub(crate) enum Version {
    V23,
    V24,
}

pub(crate) struct Frame<'a> {
    pub id: [u8; 4],
    pub flags: [u8; 2],
    pub body: Cow<'a, [u8]>,
}

struct Located {
    id: [u8; 4],
    flags: [u8; 2],
    start: usize,
    end: usize,
}

pub(crate) struct Tag<'a> {
    pub version: Version,
    pub flags: u8,
    pub footer: usize,
    body: Cow<'a, [u8]>,
    located: Vec<Located>,
    frames_unsynchronised: bool,
}

pub(crate) fn tag_span(header: &[u8; 10]) -> Option<u64> {
    if &header[..3] != b"ID3" || !matches!(header[3], 3 | 4) {
        return None;
    }
    let footer = if header[3] == 4 && header[5] & 0x10 != 0 {
        10
    } else {
        0
    };
    Some(u64::from(syncsafe(&header[6..10])?) + 10 + footer)
}

impl<'a> Tag<'a> {
    pub(crate) fn parse(tag: &'a [u8]) -> Option<Self> {
        let version = if tag[3] == 3 {
            Version::V23
        } else {
            Version::V24
        };
        let flags = tag[5];
        let footer = if version == Version::V24 && flags & 0x10 != 0 {
            10
        } else {
            0
        };
        let end = tag.len().checked_sub(footer).filter(|end| *end >= 10)?;
        let mut body = Cow::Borrowed(&tag[10..end]);
        let tag_unsynchronised = flags & 0x80 != 0;
        if tag_unsynchronised && version == Version::V23 {
            body = Cow::Owned(deunsynchronise(&body));
        }
        if flags & 0x40 != 0 {
            let skip = extended_header(version, &body)?;
            body = Cow::Owned(body[skip..].to_vec());
        }
        let located = locate(version, &body)?;
        Some(Self {
            version,
            flags,
            footer,
            body,
            located,
            frames_unsynchronised: version == Version::V24 && tag_unsynchronised,
        })
    }

    pub(crate) fn frames(&self) -> Vec<Frame<'_>> {
        self.located
            .iter()
            .map(|located| {
                let mut frame = Frame {
                    id: located.id,
                    flags: located.flags,
                    body: Cow::Borrowed(&self.body[located.start..located.end]),
                };
                if self.frames_unsynchronised {
                    frame.body = Cow::Owned(deunsynchronise_frame(frame.flags[1], &frame.body));
                    frame.flags[1] &= !0x02;
                }
                frame
            })
            .collect()
    }
}

fn locate(version: Version, body: &[u8]) -> Option<Vec<Located>> {
    let mut located = Vec::new();
    let mut offset = 0;
    while offset + FRAME_HEADER <= body.len() {
        let header = &body[offset..offset + FRAME_HEADER];
        let id = [header[0], header[1], header[2], header[3]];
        if !id
            .iter()
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit())
        {
            break;
        }
        let size = match version {
            Version::V23 => Some(u32::from_be_bytes([
                header[4], header[5], header[6], header[7],
            ])),
            Version::V24 => syncsafe(&header[4..8]),
        };
        let Some(size) = size.map(|size| size as usize) else {
            break;
        };
        let start = offset + FRAME_HEADER;
        let Some(end) = start.checked_add(size).filter(|end| *end <= body.len()) else {
            break;
        };
        offset = end;
        if size == 0 {
            continue;
        }
        if located.len() == MAX_FRAMES {
            return None;
        }
        located.push(Located {
            id,
            flags: [header[8], header[9]],
            start,
            end,
        });
    }
    Some(located)
}

fn deunsynchronise_frame(flags: u8, body: &[u8]) -> Vec<u8> {
    let prefix = usize::from(flags & 0x40 != 0)
        + usize::from(flags & 0x04 != 0)
        + if flags & 0x01 != 0 { 4 } else { 0 };
    let (head, data) = body.split_at(prefix.min(body.len()));
    let mut output = head.to_vec();
    output.extend(deunsynchronise(data));
    output
}

fn extended_header(version: Version, body: &[u8]) -> Option<usize> {
    let size = body.get(..4)?;
    match version {
        Version::V23 => Some(u32::from_be_bytes([size[0], size[1], size[2], size[3]]) as usize + 4),
        Version::V24 => Some(syncsafe(size)? as usize),
    }
    .filter(|length| *length <= body.len())
}

pub(crate) fn deunsynchronise(bytes: &[u8]) -> Vec<u8> {
    let mut output = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        output.push(bytes[index]);
        if bytes[index] == 0xff && bytes.get(index + 1) == Some(&0) {
            index += 1;
        }
        index += 1;
    }
    output
}

pub(crate) fn syncsafe(bytes: &[u8]) -> Option<u32> {
    if bytes.iter().any(|byte| byte & 0x80 != 0) {
        return None;
    }
    Some(
        bytes
            .iter()
            .fold(0, |value, byte| (value << 7) | u32::from(*byte)),
    )
}

pub(crate) fn inflate(compressed: &[u8], declared: usize) -> Option<Vec<u8>> {
    let mut output = Vec::with_capacity(declared);
    ZlibDecoder::new(compressed)
        .take(declared as u64 + 1)
        .read_to_end(&mut output)
        .ok()?;
    (output.len() == declared).then_some(output)
}
