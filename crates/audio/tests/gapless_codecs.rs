use hito_audio::Decoder;
use hito_audio::test_support::m4a::{Declared, aac_tone, append_to_moov, atom};
use std::{fs::File, path::Path, process::Command, time::Duration};

const FRAMES: usize = 24_000;

fn encode(path: &Path, codec: &str, extra: &[&str]) {
    let result = Command::new("ffmpeg")
        .args([
            "-nostdin",
            "-hide_banner",
            "-loglevel",
            "error",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=440:sample_rate=48000:duration=0.5",
            "-c:a",
            codec,
        ])
        .args(extra)
        .arg("-y")
        .arg(path)
        .output()
        .unwrap();
    assert!(result.status.success(), "fixture encoder {codec} failed");
}

fn decoded_frames(path: &Path) -> usize {
    let mut decoder = Decoder::open(File::open(path).unwrap()).unwrap();
    let mut frames = 0;
    while let Some(block) = decoder.read().unwrap() {
        frames += block.samples.len();
    }
    frames
}

#[test]
fn lossless_and_gapless_tagged_streams_decode_to_exactly_the_encoded_length() {
    let root = tempfile::tempdir().unwrap();
    for (name, codec, extra) in [
        ("exact.wav", "pcm_s16le", vec![]),
        ("exact.flac", "flac", vec![]),
        ("exact.m4a", "alac", vec![]),
        ("lame.mp3", "libmp3lame", vec![]),
        ("exact.ogg", "libvorbis", vec![]),
    ] {
        let path = root.path().join(name);
        encode(&path, codec, &extra);
        assert_eq!(decoded_frames(&path), FRAMES, "{name} is not sample exact");
    }
}

#[test]
fn encoder_delay_and_padding_without_usable_metadata_remain_in_the_decoded_stream() {
    let root = tempfile::tempdir().unwrap();
    for (name, codec, extra, limit) in [
        (
            "untagged.mp3",
            "libmp3lame",
            vec!["-write_xing", "0"],
            1_700,
        ),
        ("priming.aac", "aac", vec!["-f", "adts"], 2_112),
    ] {
        let path = root.path().join(name);
        encode(&path, codec, &extra);
        let extra_frames = decoded_frames(&path) - FRAMES;
        assert!(
            (1..=limit).contains(&extra_frames),
            "{name} carried {extra_frames} extra frames"
        );
    }
}

const SMPB: Declared = Declared::ItunesSmpb {
    priming: 1_024,
    padding: 576,
    length: FRAMES as u64,
};

fn aac_file(root: &Path, name: &str, declared: Declared) -> std::path::PathBuf {
    let path = root.join(name);
    aac_tone(&path, declared);
    path
}

fn decoded(path: &Path) -> (usize, Option<Duration>) {
    let decoder = Decoder::open(File::open(path).unwrap()).unwrap();
    let duration = decoder.format().duration;
    drop(decoder);
    (decoded_frames(path), duration)
}

#[test]
fn aac_in_m4a_decodes_to_the_declared_length_from_an_edit_list_or_itunsmpb() {
    let root = tempfile::tempdir().unwrap();
    for (name, declared) in [("elst.m4a", Declared::EditList), ("smpb.m4a", SMPB)] {
        let path = aac_file(root.path(), name, declared);
        let (frames, duration) = decoded(&path);
        assert_eq!(frames, FRAMES, "{name} is not sample exact");
        assert_eq!(duration, Some(Duration::from_millis(500)), "{name}");
    }
}

#[test]
fn aac_in_m4a_that_declares_nothing_keeps_the_encoder_delay_and_padding() {
    let root = tempfile::tempdir().unwrap();
    let path = aac_file(root.path(), "nothing.m4a", Declared::Nothing);
    let (frames, duration) = decoded(&path);
    assert!((FRAMES + 1..=FRAMES + 2_112).contains(&frames), "{frames}");
    assert!(duration.unwrap() > Duration::from_millis(500));
}

#[test]
fn seeking_a_trimmed_m4a_uses_the_trimmed_timeline() {
    let root = tempfile::tempdir().unwrap();
    for (name, declared) in [("elst.m4a", Declared::EditList), ("smpb.m4a", SMPB)] {
        let path = aac_file(root.path(), name, declared);
        for (target, remaining) in [(250_u64, 12_000_usize), (100, 19_200), (0, 24_000)] {
            let mut decoder = Decoder::open(File::open(&path).unwrap()).unwrap();
            decoder.seek(Duration::from_millis(target)).unwrap();
            let mut frames = 0;
            let mut position = None;
            while let Some(block) = decoder.read().unwrap() {
                position.get_or_insert(block.position);
                frames += block.samples.len();
            }
            assert_eq!(frames, remaining, "{name} seek {target}");
            assert_eq!(position, Some(Duration::from_millis(target)), "{name}");
        }
        let mut decoder = Decoder::open(File::open(&path).unwrap()).unwrap();
        decoder.seek(Duration::from_millis(500)).unwrap();
        assert!(decoder.read().unwrap().is_none(), "{name}");
    }
}

#[test]
fn hostile_or_truncated_m4a_declarations_never_panic_and_leave_the_stream_untrimmed() {
    let root = tempfile::tempdir().unwrap();
    let source = aac_file(root.path(), "source.m4a", Declared::Nothing);
    let bytes = std::fs::read(&source).unwrap();
    let mut variants: Vec<(String, Vec<u8>)> = Vec::new();
    for body in [
        vec![0xFF; 40],
        vec![0; 3],
        [&[0, 0, 0, 0][..], &u32::MAX.to_be_bytes(), &[0; 64]].concat(),
    ] {
        let edts = atom(b"edts", atom(b"elst", body.clone()));
        variants.push((
            "hostile elst".into(),
            append_to_moov(&bytes, &atom(b"trak", edts)),
        ));
        let smpb = atom(
            b"udta",
            atom(b"meta", [&[0; 4][..], &atom(b"ilst", body)].concat()),
        );
        variants.push(("hostile ilst".into(), append_to_moov(&bytes, &smpb)));
    }
    let nested = (0..200).fold(atom(b"free", vec![0; 4]), |inner, _| atom(b"udta", inner));
    variants.push(("deep nesting".into(), append_to_moov(&bytes, &nested)));
    for cut in (0..bytes.len()).step_by(257) {
        variants.push((format!("cut at {cut}"), bytes[..cut].to_vec()));
    }
    for (name, data) in variants {
        let path = root.path().join("hostile.m4a");
        std::fs::write(&path, &data).unwrap();
        let Ok(mut decoder) = Decoder::open(File::open(&path).unwrap()) else {
            continue;
        };
        let mut frames = 0;
        while let Ok(Some(block)) = decoder.read() {
            frames += block.samples.len();
        }
        assert!(frames <= FRAMES + 2_112, "{name} decoded {frames}");
    }
}

#[test]
fn a_declaration_longer_than_the_stream_is_ignored_and_a_shorter_one_is_honored() {
    let root = tempfile::tempdir().unwrap();
    let too_long = Declared::ItunesSmpb {
        priming: 1_024,
        padding: 0,
        length: 10_000_000,
    };
    let path = aac_file(root.path(), "long.m4a", too_long);
    let (frames, duration) = decoded(&path);
    assert!((FRAMES + 1..=FRAMES + 2_112).contains(&frames), "{frames}");
    assert!(duration.unwrap() > Duration::from_millis(500));
    let shorter = Declared::ItunesSmpb {
        priming: 2_048,
        padding: 0,
        length: 12_000,
    };
    let path = aac_file(root.path(), "short.m4a", shorter);
    assert_eq!(decoded(&path), (12_000, Some(Duration::from_millis(250))));
}
