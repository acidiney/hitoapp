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

const OWNER: &[u8] = b"http://musicbrainz.org";
const RECORDING: &str = "a1000000-0000-4000-8000-000000000003";

fn frame(version: u8, id: &[u8; 4], flags: [u8; 2], body: &[u8]) -> Vec<u8> {
    let mut bytes = id.to_vec();
    if version == 3 {
        bytes.extend((body.len() as u32).to_be_bytes());
    } else {
        bytes.extend(syncsafe(body.len()));
    }
    bytes.extend(flags);
    bytes.extend(body);
    bytes
}

fn ufid_body(owner: &[u8], identifier: &[u8]) -> Vec<u8> {
    let mut body = owner.to_vec();
    body.push(0);
    body.extend(identifier);
    body
}

fn ufid(version: u8, flags: [u8; 2], owner: &[u8], identifier: &[u8]) -> Vec<u8> {
    frame(version, b"UFID", flags, &ufid_body(owner, identifier))
}

fn id3(version: u8, tag_flags: u8, frames: &[Vec<u8>]) -> Vec<u8> {
    let body: Vec<u8> = frames.concat();
    let mut bytes = vec![b'I', b'D', b'3', version, 0, tag_flags];
    bytes.extend(syncsafe(body.len()));
    bytes.extend(body);
    bytes
}

fn found(bytes: &[u8]) -> Option<String> {
    let directory = tempfile::tempdir().unwrap();
    let mut file = write(&directory.path().join("media"), bytes);
    neutralise(&mut file, &control()).unwrap().recording_id
}

#[test]
fn the_musicbrainz_ufid_recording_id_is_read_from_both_tag_versions_and_riff_chunks() {
    let title = |version| frame(version, b"TIT2", [0, 0], b"\x00Title");
    for version in [3, 4] {
        let tag = id3(
            version,
            0,
            &[
                title(version),
                ufid(version, [0, 0], OWNER, RECORDING.as_bytes()),
            ],
        );
        assert_eq!(found(&tag).as_deref(), Some(RECORDING), "v2.{version}");
        let mut riff = b"RIFFxxxxWAVE".to_vec();
        riff.extend(chunk(b"id3 ", &tag));
        assert_eq!(
            found(&riff).as_deref(),
            Some(RECORDING),
            "riff v2.{version}"
        );
    }
    let mut stacked = id3(4, 0, &[title(4)]);
    stacked.extend(id3(4, 0, &[ufid(4, [0, 0], OWNER, RECORDING.as_bytes())]));
    assert_eq!(found(&stacked).as_deref(), Some(RECORDING), "stacked tags");
}

#[test]
fn a_tag_level_unsynchronised_tag_still_yields_its_recording_id() {
    for version in [3, 4] {
        let tag = id3(
            version,
            0x80,
            &[ufid(version, [0, 0], OWNER, RECORDING.as_bytes())],
        );
        assert_eq!(found(&tag).as_deref(), Some(RECORDING), "v2.{version}");
    }
}

#[test]
fn the_first_musicbrainz_owned_ufid_wins_and_other_owners_are_ignored() {
    let other = "b1000000-0000-4000-8000-000000000009";
    let tag = id3(
        4,
        0,
        &[
            ufid(4, [0, 0], b"http://example.org", other.as_bytes()),
            ufid(4, [0, 0], OWNER, RECORDING.as_bytes()),
            ufid(4, [0, 0], OWNER, other.as_bytes()),
        ],
    );
    assert_eq!(found(&tag).as_deref(), Some(RECORDING));
}

#[test]
fn unusable_ufid_frames_yield_no_recording_id() {
    let long = [b'a'; 65];
    let cases: Vec<(&str, Vec<u8>)> = vec![
        (
            "other owner",
            ufid(4, [0, 0], b"http://example.org", RECORDING.as_bytes()),
        ),
        (
            "owner case differs",
            ufid(4, [0, 0], b"HTTP://MUSICBRAINZ.ORG", RECORDING.as_bytes()),
        ),
        ("owner without terminator", frame(4, b"UFID", [0, 0], OWNER)),
        ("empty identifier", ufid(4, [0, 0], OWNER, b"")),
        ("oversized identifier", ufid(4, [0, 0], OWNER, &long)),
        (
            "non text identifier",
            ufid(4, [0, 0], OWNER, &[0xff, 0xfe, 0x80, 0x00]),
        ),
        ("control characters", ufid(4, [0, 0], OWNER, b"abc\x07def")),
        (
            "v24 compressed",
            ufid(4, [0, 0x09], OWNER, RECORDING.as_bytes()),
        ),
        (
            "v24 encrypted",
            ufid(4, [0, 0x04], OWNER, RECORDING.as_bytes()),
        ),
        (
            "v24 grouped",
            ufid(4, [0, 0x40], OWNER, RECORDING.as_bytes()),
        ),
        (
            "v24 frame unsynchronised",
            ufid(4, [0, 0x02], OWNER, RECORDING.as_bytes()),
        ),
        (
            "v23 compressed",
            ufid(3, [0, 0x80], OWNER, RECORDING.as_bytes()),
        ),
        (
            "v23 encrypted",
            ufid(3, [0, 0x40], OWNER, RECORDING.as_bytes()),
        ),
        (
            "v23 grouped",
            ufid(3, [0, 0x20], OWNER, RECORDING.as_bytes()),
        ),
    ];
    for (name, frame) in cases {
        let version = if name.starts_with("v23") { 3 } else { 4 };
        assert_eq!(found(&id3(version, 0, &[frame])), None, "{name}");
    }
}

#[test]
fn an_unusable_first_musicbrainz_ufid_leaves_no_recording_id_even_when_a_later_frame_or_tag_is_valid()
 {
    let long = [b'a'; 65];
    let valid = ufid(4, [0, 0], OWNER, RECORDING.as_bytes());
    for bad in [
        ufid(4, [0, 0], OWNER, b""),
        ufid(4, [0, 0], OWNER, &long),
        ufid(4, [0, 0], OWNER, b"abc\x07"),
        ufid(4, [0, 0], OWNER, b"padded\0\0\0"),
    ] {
        assert_eq!(found(&id3(4, 0, &[bad.clone(), valid.clone()])), None);
        let mut stacked = id3(4, 0, &[bad]);
        stacked.extend(id3(4, 0, &[valid.clone()]));
        assert_eq!(found(&stacked), None, "stacked tags");
    }
    let other = ufid(4, [0, 0], b"http://example.org", b"");
    let unreadable = ufid(4, [0, 0x09], OWNER, b"x");
    assert_eq!(
        found(&id3(4, 0, &[other, unreadable, valid])).as_deref(),
        Some(RECORDING)
    );
}

fn unsync(bytes: &[u8]) -> Vec<u8> {
    let mut output = Vec::new();
    for (index, byte) in bytes.iter().enumerate() {
        output.push(*byte);
        if *byte == 0xff
            && bytes
                .get(index + 1)
                .is_none_or(|next| *next == 0 || *next >= 0xe0)
        {
            output.push(0);
        }
    }
    output
}

fn raw_tag(version: u8, flags: u8, body: &[u8], footer: bool) -> Vec<u8> {
    let mut bytes = vec![b'I', b'D', b'3', version, 0, flags];
    bytes.extend(syncsafe(body.len()));
    bytes.extend(body);
    if footer {
        bytes.extend(b"3DI");
        bytes.extend([version, 0, flags]);
        bytes.extend(syncsafe(body.len()));
    }
    bytes
}

fn good_frames(version: u8) -> Vec<u8> {
    [
        frame(version, b"TIT2", [0, 0], b"\x00Title"),
        ufid(version, [0, 0], OWNER, RECORDING.as_bytes()),
    ]
    .concat()
}

#[test]
fn extended_headers_footers_and_unsynchronised_neighbours_still_yield_the_recording_id() {
    let v23_extended = [&[0, 0, 0, 6, 0, 0, 0, 0, 0, 0][..], &good_frames(3)].concat();
    assert_eq!(
        found(&raw_tag(3, 0x40, &v23_extended, false)).as_deref(),
        Some(RECORDING)
    );
    let v24_extended = [&[0, 0, 0, 6, 1, 0][..], &good_frames(4)].concat();
    assert_eq!(
        found(&raw_tag(4, 0x40, &v24_extended, false)).as_deref(),
        Some(RECORDING)
    );
    assert_eq!(
        found(&raw_tag(4, 0x10, &good_frames(4), true)).as_deref(),
        Some(RECORDING)
    );
    let mut footed = raw_tag(4, 0x10, &good_frames(4), true);
    footed.extend(id3(4, 0, &[]));
    assert_eq!(
        found(&footed).as_deref(),
        Some(RECORDING),
        "a tag after a footer"
    );

    let neighbour = |version| frame(version, b"TXXX", [0, 0], b"\x00a\0\xff\x00\xe0\xff\xff");
    let v23 = [neighbour(3), good_frames(3)].concat();
    assert_eq!(
        found(&raw_tag(3, 0x80, &unsync(&v23), false)).as_deref(),
        Some(RECORDING)
    );
    let v24 = [neighbour(4), good_frames(4)].concat();
    let encoded = [
        frame(
            4,
            b"TXXX",
            [0, 0x02],
            &unsync(b"\x00a\0\xff\x00\xe0\xff\xff"),
        ),
        good_frames(4),
    ]
    .concat();
    assert_eq!(
        found(&raw_tag(4, 0x00, &encoded, false)).as_deref(),
        Some(RECORDING),
        "frame flag without tag flag"
    );
    assert_eq!(
        found(&raw_tag(4, 0x80, &v24, false)).as_deref(),
        Some(RECORDING)
    );
}

#[test]
fn riff_chunks_with_odd_sizes_and_truncation_are_walked_safely() {
    let tag = id3(4, 0, &[ufid(4, [0, 0], OWNER, RECORDING.as_bytes())]);
    let riff = |chunks: Vec<Vec<u8>>| [b"RIFFxxxxWAVE".to_vec(), chunks.concat()].concat();
    assert_eq!(
        found(&riff(vec![chunk(b"junk", b"abc"), chunk(b"id3 ", &tag)])).as_deref(),
        Some(RECORDING),
        "odd chunk before the tag"
    );
    let odd_tag = tag.clone();
    assert_eq!(odd_tag.len() % 2, 1);
    assert_eq!(
        found(&riff(vec![
            chunk(b"id3 ", &odd_tag),
            chunk(b"data", b"abcd")
        ]))
        .as_deref(),
        Some(RECORDING),
        "odd tag chunk with padding"
    );
    let mut trailing = riff(vec![chunk(b"id3 ", &tag)]);
    trailing.extend(b"data\x10\0");
    assert_eq!(
        found(&trailing).as_deref(),
        Some(RECORDING),
        "truncated chunk header after the tag"
    );
    let mut cut = riff(vec![chunk(b"id3 ", &tag)]);
    cut.truncate(cut.len() - 5);
    assert_eq!(found(&cut), None, "chunk larger than the file");
    let mut hostile = riff(vec![chunk(b"junk", b"abc")]);
    hostile.extend(b"zzzz");
    hostile.extend(u32::MAX.to_le_bytes());
    hostile.extend(&tag);
    assert_eq!(found(&hostile), None, "declared size beyond the file");
}

#[test]
fn malformed_tag_headers_never_panic_and_pin_which_mutations_survive() {
    let good = id3(
        4,
        0,
        &[
            frame(4, b"TIT2", [0, 0], b"\x00Title"),
            ufid(4, [0, 0], OWNER, RECORDING.as_bytes()),
        ],
    );
    let title_body = 20..20 + 6;
    let identifier = good.len() - RECORDING.len()..good.len();
    let mut cases: Vec<(String, Vec<u8>)> = vec![("empty".into(), vec![])];
    for length in 0..good.len() {
        cases.push((format!("truncated at {length}"), good[..length].to_vec()));
    }
    for position in 0..good.len() {
        for value in [0x00, 0x7f, 0x80, 0xff] {
            let mut bytes = good.clone();
            bytes[position] = value;
            cases.push((format!("byte {position} set to {value:#x}"), bytes));
        }
    }
    for size in [0u32, 1, 9, 10, 0x0fff_ffff] {
        let mut bytes = good.clone();
        bytes[6..10].copy_from_slice(&syncsafe(size as usize));
        cases.push((format!("declared tag size {size}"), bytes));
    }
    for (name, bytes) in cases {
        let result = found(&bytes);
        let position = name
            .strip_prefix("byte ")
            .and_then(|rest| rest.split(' ').next())
            .and_then(|position| position.parse::<usize>().ok());
        match position {
            Some(position) if title_body.contains(&position) => {
                assert_eq!(result.as_deref(), Some(RECORDING), "{name}");
            }
            Some(position) if identifier.contains(&position) => {
                let mutated = std::str::from_utf8(&bytes[identifier.clone()]).ok();
                assert!(
                    result.is_none() || result.as_deref() == mutated,
                    "{name}: {result:?}"
                );
            }
            _ => assert!(
                result.is_none() || result.as_deref() == Some(RECORDING),
                "{name}"
            ),
        }
    }
    assert_eq!(found(&good[..good.len() - 1]), None);
    assert_eq!(found(&good).as_deref(), Some(RECORDING));
}

#[test]
fn every_tag_flag_value_and_frame_size_of_a_v23_tag_is_survived() {
    let good = id3(
        3,
        0,
        &[
            frame(3, b"TIT2", [0, 0], b"\x00Title"),
            ufid(3, [0, 0], OWNER, RECORDING.as_bytes()),
        ],
    );
    for flags in 0..=255u8 {
        let mut bytes = good.clone();
        bytes[5] = flags;
        let result = found(&bytes);
        if flags & 0x40 == 0 {
            assert_eq!(result.as_deref(), Some(RECORDING), "flags {flags:#x}");
        } else {
            assert!(
                result.is_none() || result.as_deref() == Some(RECORDING),
                "flags {flags:#x}"
            );
        }
    }
    for size in [0u32, 1, 0xffff_ffff, 0x7fff_ffff] {
        let mut bytes = good.clone();
        bytes[20 - 6..20 - 2].copy_from_slice(&size.to_be_bytes());
        let result = found(&bytes);
        assert!(
            result.is_none() || result.as_deref() == Some(RECORDING),
            "frame size {size}"
        );
    }
    for version in [3u8, 4] {
        for tail in [
            &[0xffu8][..],
            &[0xff, 0xff, 0xff],
            &[0xff, 0x00, 0xff],
            &[0xe0, 0xff],
        ] {
            let mut body = good_frames(version);
            body.extend(tail);
            let result = found(&raw_tag(version, 0x80, &body, false));
            assert!(
                result.is_none() || result.as_deref() == Some(RECORDING),
                "v2.{version} tail {tail:?}"
            );
        }
    }
}
