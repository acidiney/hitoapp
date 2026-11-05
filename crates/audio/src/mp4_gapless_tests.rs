use crate::mp4_gapless::{Mp4Timing, Trim, read_mp4_timing};
use crate::test_support::m4a::{atom, itunes_smpb_box};
use std::io::Cursor;

fn full(version: u8, fields: &[&[u8]]) -> Vec<u8> {
    [&[version, 0, 0, 0][..], &fields.concat()].concat()
}

fn mvhd(timescale: u32) -> Vec<u8> {
    atom(
        b"mvhd",
        full(0, &[&[0; 8], &timescale.to_be_bytes(), &[0; 80]]),
    )
}

fn mdhd(timescale: u32) -> Vec<u8> {
    atom(
        b"mdhd",
        full(0, &[&[0; 8], &timescale.to_be_bytes(), &[0; 8]]),
    )
}

fn hdlr(kind: &[u8; 4]) -> Vec<u8> {
    atom(b"hdlr", full(0, &[&[0; 4], kind, &[0; 12]]))
}

fn elst(entries: &[(u32, i32)]) -> Vec<u8> {
    let mut body = vec![0, 0, 0, 0];
    body.extend_from_slice(&(entries.len() as u32).to_be_bytes());
    for (duration, media_time) in entries {
        body.extend_from_slice(&duration.to_be_bytes());
        body.extend_from_slice(&media_time.to_be_bytes());
        body.extend_from_slice(&[0, 1, 0, 0]);
    }
    atom(b"elst", body)
}

fn trak(handler: &[u8; 4], media_timescale: u32, edits: Option<Vec<u8>>) -> Vec<u8> {
    let mut parts = Vec::new();
    if let Some(edits) = edits {
        parts.push(atom(b"edts", edits));
    }
    parts.push(atom(
        b"mdia",
        [mdhd(media_timescale), hdlr(handler)].concat(),
    ));
    atom(b"trak", parts.concat())
}

fn file(moov_children: &[Vec<u8>]) -> Vec<u8> {
    let ftyp = atom(b"ftyp", b"M4A \0\0\0\0M4A isom".to_vec());
    let mdat = atom(b"mdat", vec![7; 64]);
    [ftyp, mdat, atom(b"moov", moov_children.concat())].concat()
}

fn read(bytes: Vec<u8>) -> Option<Mp4Timing> {
    read_mp4_timing(&mut Cursor::new(bytes))
}

fn trim_of(bytes: Vec<u8>, rate: u32) -> Option<Trim> {
    read(bytes)?.trim(rate)
}

#[test]
fn an_edit_list_gives_the_priming_and_length_in_frames() {
    let bytes = file(&[
        mvhd(1000),
        trak(b"soun", 48_000, Some(elst(&[(500, 1_024)]))),
    ]);
    assert_eq!(
        trim_of(bytes, 48_000),
        Some(Trim {
            start: 1_024,
            length: 24_000
        })
    );
}

#[test]
fn a_media_timescale_that_differs_from_the_sample_rate_is_converted() {
    let bytes = file(&[mvhd(1000), trak(b"soun", 24_000, Some(elst(&[(500, 512)])))]);
    assert_eq!(
        trim_of(bytes, 48_000),
        Some(Trim {
            start: 1_024,
            length: 24_000
        })
    );
}

#[test]
fn itunsmpb_wins_over_an_edit_list_that_only_differs_by_rounding() {
    let smpb = itunes_smpb_box(
        " 00000000 00000400 00000240 0000000000005DC1 00000000 00000000 00000000 00000000",
    );
    let bytes = file(&[
        mvhd(1000),
        trak(b"soun", 48_000, Some(elst(&[(500, 1_024)]))),
        smpb,
    ]);
    assert_eq!(
        trim_of(bytes, 48_000),
        Some(Trim {
            start: 1_024,
            length: 24_001
        })
    );
}

#[test]
fn a_disagreeing_itunsmpb_yields_to_the_edit_list() {
    let smpb = itunes_smpb_box(" 00000000 00000100 00000240 0000000000005DC0");
    let bytes = file(&[
        mvhd(1000),
        trak(b"soun", 48_000, Some(elst(&[(500, 1_024)]))),
        smpb,
    ]);
    assert_eq!(
        trim_of(bytes, 48_000),
        Some(Trim {
            start: 1_024,
            length: 24_000
        })
    );
}

#[test]
fn itunsmpb_alone_declares_the_trim() {
    let smpb = itunes_smpb_box(" 00000000 00000840 000001C0 0000000000005DC0 00000000");
    let bytes = file(&[mvhd(1000), trak(b"soun", 44_100, None), smpb]);
    assert_eq!(
        trim_of(bytes, 44_100),
        Some(Trim {
            start: 2_112,
            length: 24_000
        })
    );
}

#[test]
fn files_without_declarations_or_audio_tracks_are_left_alone() {
    assert_eq!(
        trim_of(file(&[mvhd(1000), trak(b"soun", 48_000, None)]), 48_000),
        None
    );
    let video = trak(b"vide", 48_000, Some(elst(&[(500, 1_024)])));
    assert_eq!(trim_of(file(&[mvhd(1000), video]), 48_000), None);
}

#[test]
fn edit_lists_that_move_or_repeat_media_are_not_used() {
    for entries in [
        vec![(500, -1), (500, 1_024)],
        vec![(250, 1_024), (250, 4_096)],
        vec![(0, 1_024)],
        vec![(500, -1)],
    ] {
        let bytes = file(&[mvhd(1000), trak(b"soun", 48_000, Some(elst(&entries)))]);
        assert_eq!(trim_of(bytes, 48_000), None, "{entries:?}");
    }
}

#[test]
fn zero_timescales_are_rejected_instead_of_dividing() {
    for (movie, media) in [(0, 48_000), (1000, 0)] {
        let bytes = file(&[
            mvhd(movie),
            trak(b"soun", media, Some(elst(&[(500, 1_024)]))),
        ]);
        assert_eq!(trim_of(bytes, 48_000), None);
    }
}

#[test]
fn malformed_itunsmpb_text_is_ignored() {
    for text in [
        "",
        " 00000000",
        " 00000000 ZZZZ 0 0",
        " 00000000 00000840 000001C0 0000000000000000",
        " 00000000 FFFFFFFFFFFFFFFFFF 0 5DC0",
    ] {
        let bytes = file(&[
            mvhd(1000),
            trak(b"soun", 48_000, None),
            itunes_smpb_box(text),
        ]);
        assert_eq!(trim_of(bytes, 48_000), None, "{text:?}");
    }
}

#[test]
fn non_mp4_and_hostile_structures_are_rejected_without_panicking() {
    assert!(read(Vec::new()).is_none());
    assert!(read(b"RIFF\0\0\0\0WAVEfmt ".to_vec()).is_none());
    assert!(read(vec![0xFF; 4096]).is_none());
    let ftyp = atom(b"ftyp", b"M4A \0\0\0\0".to_vec());
    let oversize = [
        ftyp.clone(),
        0xFFFF_FF00_u32.to_be_bytes().to_vec(),
        b"moov".to_vec(),
    ]
    .concat();
    assert!(read(oversize).is_none());
    let undersize = [ftyp.clone(), 4_u32.to_be_bytes().to_vec(), b"moov".to_vec()].concat();
    assert!(read(undersize).is_none());
    let wide_overflow = [
        ftyp.clone(),
        1_u32.to_be_bytes().to_vec(),
        b"moov".to_vec(),
        u64::MAX.to_be_bytes().to_vec(),
    ]
    .concat();
    assert!(read(wide_overflow).is_none());
    let wide_short = [
        ftyp.clone(),
        1_u32.to_be_bytes().to_vec(),
        b"moov".to_vec(),
        8_u64.to_be_bytes().to_vec(),
    ]
    .concat();
    assert!(read(wide_short).is_none());
    let child_past_parent = [
        ftyp,
        atom(
            b"moov",
            [1000_u32.to_be_bytes().to_vec(), b"trak".to_vec()].concat(),
        ),
    ]
    .concat();
    assert!(read(child_past_parent).is_none());
}

#[test]
fn every_truncation_of_a_valid_file_is_handled() {
    let smpb = itunes_smpb_box(" 00000000 00000400 00000240 0000000000005DC0");
    let bytes = file(&[
        mvhd(1000),
        trak(b"soun", 48_000, Some(elst(&[(500, 1_024)]))),
        atom(b"udta", smpb),
    ]);
    for cut in 0..bytes.len() {
        let _ = read(bytes[..cut].to_vec());
    }
    for index in 0..bytes.len() {
        let mut flipped = bytes.clone();
        flipped[index] ^= 0xFF;
        let _ = read(flipped).map(|timing| timing.trim(48_000));
    }
}

#[test]
fn oversized_leaves_and_too_many_edits_are_not_read() {
    let big = atom(b"elst", vec![0; 8_192]);
    assert!(read(file(&[mvhd(1000), trak(b"soun", 48_000, Some(big))])).is_none());
    let many: Vec<(u32, i32)> = (0..100).map(|index| (1, index)).collect();
    let bytes = file(&[mvhd(1000), trak(b"soun", 48_000, Some(elst(&many)))]);
    assert_eq!(trim_of(bytes, 48_000), None);
}

#[test]
fn a_flood_of_sibling_atoms_stops_at_the_box_limit() {
    let flood: Vec<u8> = (0..10_000)
        .flat_map(|_| atom(b"free", Vec::new()))
        .collect();
    let bytes = [atom(b"ftyp", b"M4A \0\0\0\0".to_vec()), flood].concat();
    assert!(read(bytes).is_none());
}

#[test]
fn deep_nesting_does_not_recurse_past_the_known_scopes() {
    let nested = (0..5_000).fold(atom(b"free", Vec::new()), |inner, _| atom(b"udta", inner));
    let bytes = file(&[mvhd(1000), trak(b"soun", 48_000, None), nested]);
    assert_eq!(trim_of(bytes, 48_000), None);
}
