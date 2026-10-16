mod support;

use hito_audio::Decoder;

#[test]
fn an_owned_wave_decodes_into_exact_interleaved_samples() {
    let input = support::wave(48_000, 2, 960, |frame, channel| {
        ((frame % 97) as i16 - 48) * 300 + channel as i16 * 100
    });
    let mut decoder = Decoder::open(input).unwrap();
    let format = decoder.format();
    assert_eq!(format.sample_rate, 48_000);
    assert_eq!(format.channels, 2);
    let mut samples = Vec::new();
    while let Some(block) = decoder.read().unwrap() {
        samples.extend_from_slice(block.samples);
    }
    assert_eq!(samples.len(), 960 * 2);
    for (index, value) in samples.into_iter().enumerate() {
        let expected =
            (((index / 2 % 97) as i16 - 48) * 300 + (index % 2) as i16 * 100) as f32 / 32768.;
        assert_eq!(value, expected);
    }
}

#[test]
fn seeks_discard_preroll_and_honor_backward_and_zero_targets() {
    let mut decoder = Decoder::open(support::wave(48_000, 1, 144_000, |frame, _| {
        (frame % 30_001) as i16
    }))
    .unwrap();
    for target in [95_437_u64, 21_013, 0, 48_000] {
        let position = std::time::Duration::from_secs_f64(target as f64 / 48_000.);
        decoder.seek(position).unwrap();
        let block = decoder.read().unwrap().unwrap();
        assert!((block.position.as_secs_f64() - position.as_secs_f64()).abs() < 1. / 48_000.);
        assert_eq!(block.samples[0], (target % 30_001) as f32 / 32768.);
    }
}

#[test]
fn nonfinite_pcm_is_rejected_before_it_can_reach_a_device() {
    use std::io::{Seek, SeekFrom, Write};
    for sample in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        let mut input = support::wave(48_000, 1, 2, |_, _| 0);
        input.seek(SeekFrom::Start(20)).unwrap();
        input.write_all(&3_u16.to_le_bytes()).unwrap();
        input.seek(SeekFrom::Start(28)).unwrap();
        input.write_all(&192_000_u32.to_le_bytes()).unwrap();
        input.write_all(&4_u16.to_le_bytes()).unwrap();
        input.write_all(&32_u16.to_le_bytes()).unwrap();
        input.seek(SeekFrom::Start(44)).unwrap();
        input.write_all(&sample.to_le_bytes()).unwrap();
        input.seek(SeekFrom::Start(0)).unwrap();
        assert!(matches!(
            Decoder::open(input),
            Err(hito_audio::Error::Decode)
        ));
    }
}

#[test]
fn truncated_pcm_is_not_reported_as_a_successful_end() {
    let input = support::wave(48_000, 2, 48_000, |_, _| 1);
    input.set_len(2000).unwrap();
    let mut decoder = match Decoder::open(input) {
        Ok(decoder) => decoder,
        Err(_) => return,
    };
    loop {
        match decoder.read() {
            Ok(Some(_)) => (),
            Err(_) => break,
            Ok(None) => panic!("a truncated file ended without reporting failure"),
        }
    }
}
