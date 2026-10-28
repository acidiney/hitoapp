use super::{super::Control, inflate};
use std::borrow::Cow;

const MAX_FRAME_BYTES: usize = 1024 * 1024;
const MAX_TOTAL_BYTES: usize = 4 * 1024 * 1024;
const MAX_RATIO: usize = 100;
const RATIO_FREE_BYTES: usize = 4096;
const MAX_FRAMES: usize = 8192;
const FRAME_HEADER: usize = 10;

#[derive(Default)]
pub(in crate::library_storage::inspection) struct Report {
    pub decompressed: u32,
    pub ignored_over_limit: u32,
    pub ignored_invalid: u32,
    pub ignored_unsupported: u32,
    pub io_failed: u32,
    pub(super) charged: usize,
}

impl Report {
    pub(in crate::library_storage::inspection) fn ignored(&self) -> u32 {
        self.ignored_over_limit + self.ignored_invalid + self.ignored_unsupported
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Version {
    V23,
    V24,
}

struct Frame<'a> {
    id: [u8; 4],
    flags: [u8; 2],
    body: Cow<'a, [u8]>,
}

enum Ignored {
    OverLimit,
    Invalid,
    Unsupported,
}

pub(super) fn span(header: &[u8; 10]) -> Option<u64> {
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

/// Returns the replacement for a tag that contains compressed frames, with the
/// same length as the original, or `None` when the tag should stay untouched.
pub(super) fn normalise(
    tag: &[u8],
    report: &mut Report,
    control: &Control,
) -> crate::Result<Option<Vec<u8>>> {
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
    let Some(end) = tag.len().checked_sub(footer).filter(|end| *end >= 10) else {
        return Ok(None);
    };
    let mut body = Cow::Borrowed(&tag[10..end]);
    let tag_unsynchronised = flags & 0x80 != 0;
    if tag_unsynchronised && version == Version::V23 {
        body = Cow::Owned(deunsynchronise(&body));
    }
    if flags & 0x40 != 0 {
        let Some(skip) = extended_header(version, &body) else {
            return Ok(None);
        };
        body = Cow::Owned(body[skip..].to_vec());
    }
    let Some(frames) = frames(version, &body, tag_unsynchronised) else {
        return Ok(None);
    };
    if !frames.iter().any(|frame| compressed(version, frame)) {
        return Ok(None);
    }
    let capacity = tag.len() - 10 - footer;
    let mut used: usize = frames
        .iter()
        .filter(|frame| !compressed(version, frame))
        .map(|frame| FRAME_HEADER + frame.body.len())
        .sum();
    let mut output = Vec::with_capacity(capacity);
    for frame in &frames {
        control.check()?;
        if !compressed(version, frame) {
            push(&mut output, version, frame.id, frame.flags, &frame.body);
            continue;
        }
        match decompress(version, frame, report) {
            Ok(text) if used + FRAME_HEADER + text.len() <= capacity => {
                used += FRAME_HEADER + text.len();
                report.decompressed += 1;
                push(&mut output, version, frame.id, [0, 0], &text);
            }
            Ok(_) | Err(Ignored::OverLimit) => report.ignored_over_limit += 1,
            Err(Ignored::Invalid) => report.ignored_invalid += 1,
            Err(Ignored::Unsupported) => report.ignored_unsupported += 1,
        }
    }
    output.resize(capacity, 0);
    let mut replacement = tag[..10].to_vec();
    replacement[5] = flags & 0x1f;
    replacement.extend(output);
    if footer > 0 {
        let mut trailer = tag[end..].to_vec();
        trailer[5] = replacement[5];
        replacement.extend(trailer);
    }
    Ok(Some(replacement))
}

fn frames(version: Version, body: &[u8], tag_unsynchronised: bool) -> Option<Vec<Frame<'_>>> {
    let mut frames = Vec::new();
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
        if frames.len() == MAX_FRAMES {
            return None;
        }
        let mut frame = Frame {
            id,
            flags: [header[8], header[9]],
            body: Cow::Borrowed(&body[start..end]),
        };
        if version == Version::V24 && tag_unsynchronised {
            frame.body = Cow::Owned(deunsynchronise_frame(frame.flags[1], &frame.body));
            frame.flags[1] &= !0x02;
        }
        frames.push(frame);
    }
    Some(frames)
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

fn compressed(version: Version, frame: &Frame) -> bool {
    match version {
        Version::V23 => frame.flags[1] & 0x80 != 0,
        Version::V24 => frame.flags[1] & 0x08 != 0,
    }
}

fn decompress(version: Version, frame: &Frame, report: &mut Report) -> Result<Vec<u8>, Ignored> {
    let flags = frame.flags[1];
    let extra = match version {
        Version::V23 => flags & 0x60,
        Version::V24 => flags & 0x44,
    };
    if frame.id[0] != b'T' || extra != 0 {
        return Err(Ignored::Unsupported);
    }
    if version == Version::V24 && flags & 0x01 == 0 {
        return Err(Ignored::Invalid);
    }
    let (prefix, rest) = frame.body.split_at_checked(4).ok_or(Ignored::Invalid)?;
    let declared = match version {
        Version::V23 => u32::from_be_bytes([prefix[0], prefix[1], prefix[2], prefix[3]]),
        Version::V24 => syncsafe(prefix).ok_or(Ignored::Invalid)?,
    } as usize;
    if declared == 0 {
        return Err(Ignored::Invalid);
    }
    let compressed = if version == Version::V24 && flags & 0x02 != 0 {
        Cow::Owned(deunsynchronise(rest))
    } else {
        Cow::Borrowed(rest)
    };
    if declared > MAX_FRAME_BYTES
        || report.charged + declared > MAX_TOTAL_BYTES
        || (declared > RATIO_FREE_BYTES && declared > compressed.len() * MAX_RATIO)
    {
        return Err(Ignored::OverLimit);
    }
    report.charged += declared;
    inflate(&compressed, declared).ok_or(Ignored::Invalid)
}

fn push(output: &mut Vec<u8>, version: Version, id: [u8; 4], flags: [u8; 2], body: &[u8]) {
    output.extend_from_slice(&id);
    match version {
        Version::V23 => output.extend_from_slice(&(body.len() as u32).to_be_bytes()),
        Version::V24 => output.extend_from_slice(&to_syncsafe(body.len() as u32)),
    }
    output.extend_from_slice(&flags);
    output.extend_from_slice(body);
}

fn extended_header(version: Version, body: &[u8]) -> Option<usize> {
    let size = body.get(..4)?;
    match version {
        Version::V23 => Some(u32::from_be_bytes([size[0], size[1], size[2], size[3]]) as usize + 4),
        Version::V24 => Some(syncsafe(size)? as usize),
    }
    .filter(|length| *length <= body.len())
}

fn deunsynchronise(bytes: &[u8]) -> Vec<u8> {
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

fn syncsafe(bytes: &[u8]) -> Option<u32> {
    if bytes.iter().any(|byte| byte & 0x80 != 0) {
        return None;
    }
    Some(
        bytes
            .iter()
            .fold(0, |value, byte| (value << 7) | u32::from(*byte)),
    )
}

fn to_syncsafe(value: u32) -> [u8; 4] {
    [
        (value >> 21) as u8 & 0x7f,
        (value >> 14) as u8 & 0x7f,
        (value >> 7) as u8 & 0x7f,
        value as u8 & 0x7f,
    ]
}
