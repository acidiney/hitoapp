use super::{Control, neutralise};
use std::{
    fs::{File, OpenOptions},
    io::{Read, Write},
    path::Path,
    sync::{Arc, atomic::AtomicBool},
    time::{Duration, Instant},
};

fn control() -> Control {
    Control {
        cancelled: Arc::new(AtomicBool::new(false)),
        started: Instant::now(),
        timeout: Duration::from_secs(30),
    }
}

fn syncsafe(value: usize) -> [u8; 4] {
    [
        (value >> 21) as u8 & 0x7f,
        (value >> 14) as u8 & 0x7f,
        (value >> 7) as u8 & 0x7f,
        value as u8 & 0x7f,
    ]
}

fn failing_frame() -> Vec<u8> {
    let mut body = syncsafe(1024 * 1024).to_vec();
    body.extend([0x78, 0x9c, 7].repeat(6000));
    let mut frame = b"TIT2".to_vec();
    frame.extend(syncsafe(body.len()));
    frame.extend([0, 0x09]);
    frame.extend(body);
    frame
}

fn tag(frames: usize) -> Vec<u8> {
    let body: Vec<u8> = (0..frames).flat_map(|_| failing_frame()).collect();
    let mut bytes = b"ID3\x04\0\0".to_vec();
    bytes.extend(syncsafe(body.len()));
    bytes.extend(body);
    bytes
}

fn write(path: &Path, bytes: &[u8]) -> File {
    let mut file = OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .open(path)
        .unwrap();
    file.write_all(bytes).unwrap();
    file
}

fn chunk(id: &[u8; 4], body: &[u8]) -> Vec<u8> {
    let mut bytes = id.to_vec();
    bytes.extend((body.len() as u32).to_le_bytes());
    bytes.extend(body);
    if body.len() % 2 == 1 {
        bytes.push(0);
    }
    bytes
}

#[test]
fn failing_frames_are_charged_before_inflating_across_stacked_tags() {
    let directory = tempfile::tempdir().unwrap();
    let mut bytes = tag(3);
    bytes.extend(tag(3));
    let mut file = write(&directory.path().join("media"), &bytes);
    let report = neutralise(&mut file, &control()).unwrap();
    assert_eq!(report.ignored_invalid, 4);
    assert_eq!(report.ignored_over_limit, 2);
}

#[test]
fn a_tag_or_chunk_larger_than_the_file_is_left_alone() {
    let directory = tempfile::tempdir().unwrap();
    let mut truncated = tag(1);
    truncated[6..10].copy_from_slice(&syncsafe(1024 * 1024));
    let mut file = write(&directory.path().join("tag"), &truncated);
    let report = neutralise(&mut file, &control()).unwrap();
    assert_eq!(report.ignored() + report.decompressed + report.io_failed, 0);

    let mut riff = b"RIFFxxxxWAVE".to_vec();
    riff.extend(b"id3 ");
    riff.extend(u32::MAX.to_le_bytes());
    riff.extend(tag(1));
    let mut file = write(&directory.path().join("riff"), &riff);
    let report = neutralise(&mut file, &control()).unwrap();
    assert_eq!(report.ignored() + report.decompressed + report.io_failed, 0);
    let mut after = Vec::new();
    File::open(directory.path().join("riff"))
        .unwrap()
        .read_to_end(&mut after)
        .unwrap();
    assert_eq!(after, riff);
}

#[test]
fn only_info_lists_are_hidden_when_an_id3_chunk_exists() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("media");
    let mut riff = b"RIFFxxxxWAVE".to_vec();
    riff.extend(chunk(b"LIST", b"INFOISFT\x02\0\0\0a\0"));
    riff.extend(chunk(b"LIST", b"adtllabl\x04\0\0\0\x01\0\0\0"));
    riff.extend(chunk(b"ID3 ", &tag(0)));
    let mut file = write(&path, &riff);
    neutralise(&mut file, &control()).unwrap();
    let mut after = Vec::new();
    File::open(&path).unwrap().read_to_end(&mut after).unwrap();
    let text = String::from_utf8_lossy(&after);
    assert!(text.contains("JUNK"));
    assert_eq!(text.matches("LIST").count(), 1);
    assert!(text.contains("adtl"));
    assert!(text.contains("id3 "));
    assert!(!text.contains("ID3 "));
}

#[test]
fn a_failed_rewrite_leaves_the_copy_usable_and_is_counted() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("media");
    let compressed = tag(1);
    write(&path, &compressed);
    let mut read_only = File::open(&path).unwrap();
    let report = neutralise(&mut read_only, &control()).unwrap();
    assert!(report.io_failed > 0);
    let mut after = Vec::new();
    File::open(&path).unwrap().read_to_end(&mut after).unwrap();
    assert_eq!(after, compressed);
}
