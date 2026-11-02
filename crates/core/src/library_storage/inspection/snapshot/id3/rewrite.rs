use super::super::Control;
use crate::id3_frames::{FRAME_HEADER, Frame, Tag, Version, deunsynchronise, inflate, syncsafe};
use std::borrow::Cow;

const MAX_FRAME_BYTES: usize = 1024 * 1024;
const MAX_TOTAL_BYTES: usize = 4 * 1024 * 1024;
const MAX_RATIO: usize = 100;
const RATIO_FREE_BYTES: usize = 4096;

#[derive(Default)]
pub(in crate::library_storage::inspection) struct Report {
    pub decompressed: u32,
    pub ignored_over_limit: u32,
    pub ignored_invalid: u32,
    pub ignored_unsupported: u32,
    pub io_failed: u32,
    pub appended_moved: u32,
    pub appended_dropped: u32,
    pub recording_id: Option<String>,
    pub(super) recording_decided: bool,
    pub(super) charged: usize,
}

impl Report {
    pub(in crate::library_storage::inspection) fn ignored(&self) -> u32 {
        self.ignored_over_limit + self.ignored_invalid + self.ignored_unsupported
    }
}

enum Ignored {
    OverLimit,
    Invalid,
    Unsupported,
}

pub(super) fn normalise(
    tag: &[u8],
    report: &mut Report,
    control: &Control,
) -> crate::Result<Option<Vec<u8>>> {
    let Some(parsed) = Tag::parse(tag) else {
        return Ok(None);
    };
    let (version, flags, footer) = (parsed.version, parsed.flags, parsed.footer);
    let end = tag.len() - footer;
    let frames = parsed.frames();
    if !report.recording_decided
        && let Some(identifier) = frames
            .iter()
            .find_map(|frame| musicbrainz_identifier(version, frame))
    {
        report.recording_decided = true;
        report.recording_id = identifier;
    }
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

const MUSICBRAINZ_OWNER: &[u8] = b"http://musicbrainz.org";
const MAX_IDENTIFIER_BYTES: usize = 64;

fn musicbrainz_identifier(version: Version, frame: &Frame) -> Option<Option<String>> {
    let unsupported = match version {
        Version::V23 => 0xe0,
        Version::V24 => 0x4f,
    };
    if &frame.id != b"UFID" || frame.flags[1] & unsupported != 0 {
        return None;
    }
    let terminator = frame.body.iter().position(|byte| *byte == 0)?;
    if &frame.body[..terminator] != MUSICBRAINZ_OWNER {
        return None;
    }
    let identifier = &frame.body[terminator + 1..];
    let usable = !identifier.is_empty()
        && identifier.len() <= MAX_IDENTIFIER_BYTES
        && identifier.iter().all(|byte| (0x20..0x7f).contains(byte));
    Some(
        usable
            .then(|| String::from_utf8(identifier.to_vec()).ok())
            .flatten(),
    )
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

fn to_syncsafe(value: u32) -> [u8; 4] {
    [
        (value >> 21) as u8 & 0x7f,
        (value >> 14) as u8 & 0x7f,
        (value >> 7) as u8 & 0x7f,
        value as u8 & 0x7f,
    ]
}
