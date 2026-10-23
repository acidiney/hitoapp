use hito_audio::{Decoder, Error};
use std::{fs::File, path::Path, process::Command};

fn fixture(path: &Path, source: &str, codec: &str) {
    let result = Command::new("ffmpeg")
        .args([
            "-nostdin",
            "-hide_banner",
            "-loglevel",
            "error",
            "-f",
            "lavfi",
            "-i",
            source,
            "-t",
            "0.25",
            "-c:a",
            codec,
            "-y",
        ])
        .arg(path)
        .output()
        .unwrap();
    assert!(result.status.success(), "fixture encoder {codec} failed");
}

#[test]
fn reproducible_codec_fixtures_decode_real_audio() {
    let root = tempfile::tempdir().unwrap();
    for (extension, codec) in [
        ("wav", "pcm_s16le"),
        ("wav", "pcm_s24le"),
        ("wav", "pcm_f32le"),
        ("flac", "flac"),
        ("mp3", "libmp3lame"),
        ("ogg", "libvorbis"),
        ("m4a", "aac"),
        ("m4a", "alac"),
        ("aiff", "pcm_s16be"),
        ("caf", "pcm_s16le"),
    ] {
        let path = root.path().join(format!("{codec}.{extension}"));
        fixture(&path, "sine=frequency=440:sample_rate=48000", codec);
        let mut decoder = Decoder::open(File::open(&path).unwrap())
            .unwrap_or_else(|error| panic!("{codec} open failed: {error}"));
        assert_eq!(decoder.format().sample_rate, 48_000, "{codec}");
        assert_eq!(decoder.format().channels, 1, "{codec}");
        let mut samples = 0;
        let mut energy = 0.;
        while let Some(block) = decoder
            .read()
            .unwrap_or_else(|error| panic!("{codec} decode failed: {error}"))
        {
            samples += block.samples.len();
            energy += block
                .samples
                .iter()
                .map(|sample| f64::from(*sample).powi(2))
                .sum::<f64>();
        }
        assert!(
            (11_000..=14_400).contains(&samples),
            "{codec} sample count {samples}"
        );
        assert!(
            (0.005..0.01).contains(&(energy / samples as f64)),
            "{codec} signal energy"
        );
    }
}

#[test]
fn high_resolution_pcm_is_not_quantized_to_sixteen_bits() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("quiet.wav");
    fixture(
        &path,
        "aevalsrc=0.00000011920928955078125:s=48000",
        "pcm_s24le",
    );
    let mut decoder = Decoder::open(File::open(&path).unwrap()).unwrap();
    let block = decoder.read().unwrap().unwrap();
    assert!(
        block
            .samples
            .iter()
            .all(|sample| *sample == 1. / 8_388_608.)
    );
}

#[test]
fn opus_is_explicitly_unsupported_until_the_fallback_is_implemented() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("unsupported.opus");
    fixture(&path, "sine=frequency=440:sample_rate=48000", "libopus");
    assert!(matches!(
        Decoder::open(File::open(path).unwrap()),
        Err(Error::Unsupported)
    ));
}

#[test]
fn cancellation_is_checked_while_scanning_a_non_audio_tail() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("tail.mkv");
    let encoded = Command::new("ffmpeg")
        .args([
            "-nostdin",
            "-hide_banner",
            "-loglevel",
            "error",
            "-f",
            "lavfi",
            "-i",
            "testsrc=size=16x16:rate=100:duration=2",
            "-f",
            "lavfi",
            "-i",
            "sine=sample_rate=48000:duration=0.1",
            "-map",
            "1:a",
            "-map",
            "0:v",
            "-c:a",
            "pcm_s16le",
            "-c:v",
            "ffv1",
            "-y",
        ])
        .arg(&path)
        .output()
        .unwrap();
    assert!(encoded.status.success());
    let mut decoder = Decoder::open(File::open(path).unwrap()).unwrap();
    let mut frames = 0;
    while frames < 4800 {
        frames += decoder.read().unwrap().unwrap().samples.len();
    }
    let mut checks = 0;
    assert!(matches!(
        decoder.read_until(|| {
            checks += 1;
            checks >= 3
        }),
        Err(Error::Interrupted)
    ));
    assert_eq!(checks, 3);
}
