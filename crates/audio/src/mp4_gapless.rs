use std::io::{Read, Seek, SeekFrom};

const MAX_BOXES: usize = 4096;
const MAX_LEAF_BYTES: u64 = 4096;
const MAX_EDITS: usize = 64;
const SOUND_HANDLER: [u8; 4] = *b"soun";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Trim {
    pub start: u64,
    pub length: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Edit {
    duration: u64,
    media_time: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ItunesSmpb {
    priming: u64,
    length: u64,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
struct Track {
    handler: [u8; 4],
    media_timescale: u32,
    edits: Vec<Edit>,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct Mp4Timing {
    movie_timescale: u32,
    track: Track,
    smpb: Option<ItunesSmpb>,
}

impl Mp4Timing {
    pub(crate) fn trim(&self, sample_rate: u32) -> Option<Trim> {
        let rate = u128::from(sample_rate);
        let from_edits = self.edit_trim(rate);
        let from_smpb = self.smpb.filter(|smpb| smpb.length > 0).map(|smpb| Trim {
            start: smpb.priming,
            length: smpb.length,
        });
        match (from_edits, from_smpb) {
            (Some(edits), Some(smpb)) => {
                let tolerance = rate / u128::from(self.movie_timescale.max(1)) + 1;
                let agrees = edits.start == smpb.start
                    && u128::from(edits.length.abs_diff(smpb.length)) <= tolerance;
                Some(if agrees { smpb } else { edits })
            }
            (edits, smpb) => edits.or(smpb),
        }
    }

    fn edit_trim(&self, rate: u128) -> Option<Trim> {
        let [edit] = self.track.edits.as_slice() else {
            return None;
        };
        if edit.media_time < 0
            || edit.duration == 0
            || self.movie_timescale == 0
            || self.track.media_timescale == 0
        {
            return None;
        }
        Some(Trim {
            start: u64::try_from(
                (edit.media_time as u128 * rate) / u128::from(self.track.media_timescale),
            )
            .ok()?,
            length: u64::try_from(
                (u128::from(edit.duration) * rate + u128::from(self.movie_timescale) / 2)
                    / u128::from(self.movie_timescale),
            )
            .ok()?,
        })
    }
}

pub(crate) fn read_mp4_timing<R: Read + Seek>(reader: &mut R) -> Option<Mp4Timing> {
    let length = reader.seek(SeekFrom::End(0)).ok()?;
    let mut scanner = Scanner {
        reader,
        boxes: 0,
        done: false,
        movie_timescale: 0,
        current: Track::default(),
        audio: None,
        freeform_is_smpb: false,
        smpb: None,
    };
    let first = scanner.header(0, length)?;
    if first.kind != *b"ftyp" {
        return None;
    }
    scanner.walk(Scope::Top, 0, length)?;
    let audio = scanner.audio?;
    Some(Mp4Timing {
        movie_timescale: scanner.movie_timescale,
        track: audio,
        smpb: scanner.smpb,
    })
}

#[derive(Debug, Clone, Copy)]
enum Scope {
    Top,
    Moov,
    Trak,
    Edts,
    Mdia,
    Udta,
    Meta,
    Ilst,
    Freeform,
}

struct BoxHeader {
    kind: [u8; 4],
    body: u64,
    end: u64,
    clamped: bool,
}

struct Scanner<'a, R> {
    reader: &'a mut R,
    boxes: usize,
    done: bool,
    movie_timescale: u32,
    current: Track,
    audio: Option<Track>,
    freeform_is_smpb: bool,
    smpb: Option<ItunesSmpb>,
}

impl<R: Read + Seek> Scanner<'_, R> {
    fn header(&mut self, at: u64, limit: u64) -> Option<BoxHeader> {
        self.reader.seek(SeekFrom::Start(at)).ok()?;
        let mut head = [0; 8];
        self.reader.read_exact(&mut head).ok()?;
        let size = u32::from_be_bytes(head[..4].try_into().ok()?);
        let kind: [u8; 4] = head[4..].try_into().ok()?;
        let (header_len, size) = match size {
            0 => (8, limit - at),
            1 => {
                let mut wide = [0; 8];
                self.reader.read_exact(&mut wide).ok()?;
                (16, u64::from_be_bytes(wide))
            }
            size => (8, u64::from(size)),
        };
        if size < header_len {
            return None;
        }
        let end = at.checked_add(size)?;
        Some(BoxHeader {
            kind,
            body: at + header_len,
            end: end.min(limit),
            clamped: end > limit,
        })
    }

    fn leaf(&mut self, header: &BoxHeader) -> Option<Vec<u8>> {
        let length = header.end - header.body;
        if length > MAX_LEAF_BYTES || header.clamped {
            return None;
        }
        self.reader.seek(SeekFrom::Start(header.body)).ok()?;
        let mut bytes = vec![0; length as usize];
        self.reader.read_exact(&mut bytes).ok()?;
        Some(bytes)
    }

    fn walk(&mut self, scope: Scope, start: u64, end: u64) -> Option<()> {
        let mut at = start;
        while !self.done && end.checked_sub(at)? >= 8 {
            self.boxes += 1;
            if self.boxes > MAX_BOXES {
                return None;
            }
            let header = self.header(at, end)?;
            if header.clamped && !matches!(scope, Scope::Top) {
                return None;
            }
            if header.clamped && header.kind == *b"moov" {
                return None;
            }
            if header.clamped {
                break;
            }
            self.visit(scope, &header)?;
            at = header.end;
        }
        Some(())
    }

    fn visit(&mut self, scope: Scope, header: &BoxHeader) -> Option<()> {
        let (body, end) = (header.body, header.end);
        match (scope, &header.kind) {
            (Scope::Top, b"moov") => {
                self.walk(Scope::Moov, body, end)?;
                self.done = true;
            }
            (Scope::Moov, b"mvhd") => {
                self.movie_timescale = timescale(&self.leaf(header)?).unwrap_or(0);
            }
            (Scope::Moov, b"trak") => {
                self.current = Track::default();
                self.walk(Scope::Trak, body, end)?;
                if self.current.handler == SOUND_HANDLER && self.audio.is_none() {
                    self.audio = Some(std::mem::take(&mut self.current));
                }
            }
            (Scope::Moov, b"udta") => self.walk(Scope::Udta, body, end)?,
            (Scope::Trak, b"edts") => self.walk(Scope::Edts, body, end)?,
            (Scope::Trak, b"mdia") => self.walk(Scope::Mdia, body, end)?,
            (Scope::Edts, b"elst") => {
                self.current.edits = edit_list(&self.leaf(header)?).unwrap_or_default();
            }
            (Scope::Mdia, b"mdhd") => {
                self.current.media_timescale = timescale(&self.leaf(header)?).unwrap_or(0);
            }
            (Scope::Mdia, b"hdlr") => {
                if let Some(handler) = self.leaf(header)?.get(8..12) {
                    self.current.handler = handler.try_into().ok()?;
                }
            }
            (Scope::Udta, b"meta") => self.walk(Scope::Meta, body.checked_add(4)?.min(end), end)?,
            (Scope::Meta, b"ilst") => self.walk(Scope::Ilst, body, end)?,
            (Scope::Ilst, b"----") => {
                self.freeform_is_smpb = false;
                self.walk(Scope::Freeform, body, end)?;
            }
            (Scope::Freeform, b"name") => {
                self.freeform_is_smpb = self.leaf(header)?.get(4..) == Some(b"iTunSMPB");
            }
            (Scope::Freeform, b"data") if self.freeform_is_smpb && self.smpb.is_none() => {
                self.smpb = itunes_smpb(&self.leaf(header)?);
            }
            _ => {}
        }
        Some(())
    }
}

fn be32(bytes: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes(bytes.get(at..at + 4)?.try_into().ok()?))
}

fn be64(bytes: &[u8], at: usize) -> Option<u64> {
    Some(u64::from_be_bytes(bytes.get(at..at + 8)?.try_into().ok()?))
}

fn timescale(mvhd: &[u8]) -> Option<u32> {
    match mvhd.first()? {
        0 => be32(mvhd, 12),
        1 => be32(mvhd, 20),
        _ => None,
    }
}

fn edit_list(elst: &[u8]) -> Option<Vec<Edit>> {
    let wide = match elst.first()? {
        0 => false,
        1 => true,
        _ => return None,
    };
    let count = be32(elst, 4)? as usize;
    let entry = if wide { 20 } else { 12 };
    if count > MAX_EDITS || elst.len() < 8 + count * entry {
        return None;
    }
    (0..count)
        .map(|index| {
            let at = 8 + index * entry;
            Some(if wide {
                Edit {
                    duration: be64(elst, at)?,
                    media_time: be64(elst, at + 8)? as i64,
                }
            } else {
                Edit {
                    duration: u64::from(be32(elst, at)?),
                    media_time: i64::from(be32(elst, at + 4)? as i32),
                }
            })
        })
        .collect()
}

fn itunes_smpb(data: &[u8]) -> Option<ItunesSmpb> {
    let text = std::str::from_utf8(data.get(8..)?).ok()?;
    let mut fields = text.split_ascii_whitespace();
    fields.next()?;
    let mut hex = || u64::from_str_radix(fields.next()?, 16).ok();
    let priming = hex()?;
    hex()?;
    let length = hex()?;
    Some(ItunesSmpb { priming, length })
}
