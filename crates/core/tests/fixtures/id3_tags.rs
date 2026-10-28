#![allow(dead_code)]

use flate2::{Compression, write::ZlibEncoder};
use std::{io::Write, path::Path, process::Command};

#[derive(Clone, Copy, PartialEq)]
pub enum Version {
    V23,
    V24,
}

pub enum Storage {
    Plain,
    Zlib,
    ZlibDeclaring(u32),
    CompressedBytes(Vec<u8>, u32),
}

pub struct Frame {
    pub id: &'static str,
    pub payload: Vec<u8>,
    pub storage: Storage,
    pub unsynchronised: bool,
}

impl Frame {
    pub fn plain(id: &'static str, payload: Vec<u8>) -> Self {
        Self {
            id,
            payload,
            storage: Storage::Plain,
            unsynchronised: false,
        }
    }

    pub fn zlib(id: &'static str, payload: Vec<u8>) -> Self {
        Self {
            storage: Storage::Zlib,
            ..Self::plain(id, payload)
        }
    }

    pub fn text(id: &'static str, text: &str) -> Self {
        Self::plain(id, text_payload(text))
    }

    pub fn compressed_text(id: &'static str, text: &str) -> Self {
        Self::zlib(id, text_payload(text))
    }
}

pub fn text_payload(text: &str) -> Vec<u8> {
    let mut payload = vec![3];
    payload.extend_from_slice(text.as_bytes());
    payload
}

pub fn utf16_payload(text: &str) -> Vec<u8> {
    let mut payload = vec![1, 0xff, 0xfe];
    for unit in text.encode_utf16() {
        payload.extend_from_slice(&unit.to_le_bytes());
    }
    payload
}

pub fn zlib(bytes: &[u8]) -> Vec<u8> {
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::best());
    encoder.write_all(bytes).unwrap();
    encoder.finish().unwrap()
}

pub fn unsynchronise(bytes: &[u8]) -> Vec<u8> {
    let mut output = Vec::with_capacity(bytes.len());
    for (index, byte) in bytes.iter().enumerate() {
        output.push(*byte);
        if *byte == 0xff
            && bytes
                .get(index + 1)
                .is_none_or(|next| *next >= 0xe0 || *next == 0)
        {
            output.push(0);
        }
    }
    output
}

fn syncsafe(value: u32) -> [u8; 4] {
    [
        (value >> 21) as u8 & 0x7f,
        (value >> 14) as u8 & 0x7f,
        (value >> 7) as u8 & 0x7f,
        value as u8 & 0x7f,
    ]
}

fn encode_frame(version: Version, frame: &Frame, header_unsynchronised: bool) -> Vec<u8> {
    let (mut body, compressed, declared) = match &frame.storage {
        Storage::Plain => (frame.payload.clone(), false, 0),
        Storage::Zlib => (zlib(&frame.payload), true, frame.payload.len() as u32),
        Storage::ZlibDeclaring(declared) => (zlib(&frame.payload), true, *declared),
        Storage::CompressedBytes(bytes, declared) => (bytes.clone(), true, *declared),
    };
    let mut flags = [0u8; 2];
    if frame.unsynchronised {
        assert!(
            version == Version::V24,
            "frame unsynchronisation is a 2.4 flag"
        );
        body = unsynchronise(&body);
        flags[1] |= 0x02;
    }
    let mut prefix = Vec::new();
    if compressed {
        match version {
            Version::V23 => {
                flags[1] |= 0x80;
                prefix.extend_from_slice(&declared.to_be_bytes());
            }
            Version::V24 => {
                flags[1] |= 0x08 | 0x01;
                prefix.extend_from_slice(&syncsafe(declared));
            }
        }
    }
    if header_unsynchronised {
        body = unsynchronise(&body);
    }
    let size = (prefix.len() + body.len()) as u32;
    let mut bytes = frame.id.as_bytes().to_vec();
    match version {
        Version::V23 => bytes.extend_from_slice(&size.to_be_bytes()),
        Version::V24 => bytes.extend_from_slice(&syncsafe(size)),
    }
    bytes.extend_from_slice(&flags);
    bytes.extend(prefix);
    bytes.extend(body);
    bytes
}

#[derive(Clone, Copy, Default)]
pub struct Layout {
    pub unsynchronised: bool,
    pub extended_header: bool,
    pub footer: bool,
}

pub fn tag(version: Version, whole_tag_unsynchronised: bool, frames: &[Frame]) -> Vec<u8> {
    tag_with(
        version,
        Layout {
            unsynchronised: whole_tag_unsynchronised,
            ..Layout::default()
        },
        frames,
    )
}

pub fn tag_with(version: Version, layout: Layout, frames: &[Frame]) -> Vec<u8> {
    let header_unsynchronised = layout.unsynchronised && version == Version::V24;
    let mut body = Vec::new();
    if layout.extended_header {
        body.extend_from_slice(&match version {
            Version::V23 => vec![0, 0, 0, 6, 0, 0, 0, 0, 0, 0],
            Version::V24 => vec![0, 0, 0, 6, 1, 0],
        });
    }
    for frame in frames {
        body.extend(encode_frame(version, frame, header_unsynchronised));
    }
    let mut flags = 0;
    if layout.unsynchronised {
        if version == Version::V23 {
            body = unsynchronise(&body);
        }
        flags |= 0x80;
    }
    if layout.extended_header {
        flags |= 0x40;
    }
    if layout.footer {
        assert!(version == Version::V24);
        flags |= 0x10;
    }
    let version_byte = if version == Version::V23 { 3 } else { 4 };
    let size = syncsafe(body.len() as u32);
    let mut bytes = vec![b'I', b'D', b'3', version_byte, 0, flags];
    bytes.extend_from_slice(&size);
    bytes.extend(body);
    if layout.footer {
        bytes.extend_from_slice(&[b'3', b'D', b'I', version_byte, 0, flags]);
        bytes.extend_from_slice(&size);
    }
    bytes
}

pub fn append_wav_id3_chunk(path: &Path, tag: &[u8]) {
    let mut wav = std::fs::read(path).unwrap();
    wav.extend_from_slice(b"id3 ");
    wav.extend_from_slice(&(tag.len() as u32).to_le_bytes());
    wav.extend_from_slice(tag);
    if tag.len() % 2 == 1 {
        wav.push(0);
    }
    let riff = (wav.len() - 8) as u32;
    wav[4..8].copy_from_slice(&riff.to_le_bytes());
    std::fs::write(path, wav).unwrap();
}

pub fn write_mp3_with_tag(scratch: &Path, path: &Path, tag: &[u8]) {
    let source = scratch.join("id3-source.wav");
    let plain = scratch.join("id3-plain.mp3");
    crate::support::write_wav(&source, 0);
    assert!(
        Command::new("ffmpeg")
            .args(["-nostdin", "-v", "error", "-y", "-i"])
            .arg(&source)
            .args([
                "-c:a",
                "libmp3lame",
                "-write_id3v2",
                "0",
                "-write_xing",
                "0"
            ])
            .arg(&plain)
            .status()
            .unwrap()
            .success()
    );
    let mut bytes = tag.to_vec();
    bytes.extend(std::fs::read(plain).unwrap());
    std::fs::write(path, bytes).unwrap();
}

pub fn append_aiff_id3_chunk(path: &Path, tag: &[u8]) {
    let mut aiff = std::fs::read(path).unwrap();
    aiff.extend_from_slice(b"ID3 ");
    aiff.extend_from_slice(&(tag.len() as u32).to_be_bytes());
    aiff.extend_from_slice(tag);
    if tag.len() % 2 == 1 {
        aiff.push(0);
    }
    let form = (aiff.len() - 8) as u32;
    aiff[4..8].copy_from_slice(&form.to_be_bytes());
    std::fs::write(path, aiff).unwrap();
}
