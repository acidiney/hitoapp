use hito_audio::test_support as support;
use hito_audio::{AudioFormat, Converter, Decoder, Error, SpeedCapability};
use std::time::Duration;

const RANGE: SpeedCapability = SpeedCapability::Range { min: 0.5, max: 3. };

fn format(sample_rate: u32, channels: u16) -> AudioFormat {
    AudioFormat {
        sample_rate,
        channels,
        duration: None,
    }
}

fn converter(rate: u32, channels: u16, frames: u32, sample: impl Fn(u32, u16) -> i16) -> Converter {
    Converter::new(Decoder::open(support::wave(rate, channels, frames, sample)).unwrap())
}

fn tone(rate: u32, hertz: f64, amplitude: f64, frame: u32) -> i16 {
    ((f64::from(frame) * hertz * std::f64::consts::TAU / f64::from(rate)).sin() * amplitude) as i16
}

struct Converted {
    samples: Vec<f32>,
    frames: usize,
}

fn drain(converter: &mut Converter) -> Converted {
    let channels = usize::from(converter.format().channels);
    let rate = f64::from(converter.format().sample_rate);
    let mut samples = Vec::new();
    while let Some(block) = converter.read().unwrap() {
        let frames = samples.len() / channels;
        assert!(
            (block.position.as_secs_f64() * rate - frames as f64).abs() <= 1.,
            "block at {:?} is not contiguous with {frames} converted frames",
            block.position
        );
        samples.extend_from_slice(block.samples);
    }
    Converted {
        frames: samples.len() / channels,
        samples,
    }
}

fn channel(converted: &Converted, channels: usize, index: usize) -> Vec<f32> {
    converted
        .samples
        .chunks_exact(channels)
        .map(|frame| frame[index])
        .collect()
}

fn amplitude(samples: &[f32], rate: u32, hertz: f64) -> f64 {
    let (mut real, mut imaginary) = (0., 0.);
    for (index, sample) in samples.iter().enumerate() {
        let phase = index as f64 * hertz * std::f64::consts::TAU / f64::from(rate);
        real += f64::from(*sample) * phase.cos();
        imaginary += f64::from(*sample) * phase.sin();
    }
    real.hypot(imaginary) * 2. / samples.len() as f64
}

#[test]
fn speed_capability_is_derived_from_the_decoded_representation() {
    for (rate, channels) in [
        (8_000, 1),
        (44_100, 2),
        (192_000, 2),
        (352_800, 2),
        (384_000, 2),
        (48_000, 6),
        (384_000, 8),
        (48_000, 32),
    ] {
        assert_eq!(
            format(rate, channels).speed_capability(),
            RANGE,
            "{rate} Hz {channels} channels"
        );
    }
    for (rate, channels) in [(7_999, 2), (384_001, 2), (48_000, 0), (48_000, 33)] {
        let capability = format(rate, channels).speed_capability();
        assert_eq!(capability, SpeedCapability::UnitOnly);
        assert!(capability.supports(1.));
        assert!(!capability.supports(1.5));
        assert!(!capability.supports(0.5));
    }
    assert!(RANGE.supports(0.5) && RANGE.supports(3.) && RANGE.supports(1.));
    assert!(!RANGE.supports(0.49) && !RANGE.supports(3.01) && !RANGE.supports(f32::NAN));
}

#[test]
fn the_output_representation_is_the_source_unless_the_device_path_needs_conversion() {
    let duration = Some(Duration::from_secs(7));
    for (source, expected) in [
        (format(8_000, 1), format(8_000, 1)),
        (format(44_100, 2), format(44_100, 2)),
        (format(192_000, 2), format(192_000, 2)),
        (format(352_800, 2), format(176_400, 2)),
        (format(384_000, 2), format(192_000, 2)),
        (format(384_000, 1), format(192_000, 1)),
        (format(48_000, 6), format(48_000, 2)),
        (format(384_000, 6), format(192_000, 2)),
        (format(96_000, 3), format(96_000, 2)),
    ] {
        let source = AudioFormat { duration, ..source };
        assert_eq!(
            source.output_format(),
            AudioFormat {
                duration,
                ..expected
            }
        );
        assert_eq!(
            source.requires_conversion(),
            source.output_format() != source
        );
    }
}

#[test]
fn a_representation_the_device_path_accepts_is_forwarded_bit_exact() {
    let sample = |frame: u32, channel: u16| ((frame % 97) as i16 - 48) * 300 + channel as i16 * 100;
    let mut native = Decoder::open(support::wave(96_000, 2, 9_600, sample)).unwrap();
    let mut converted = converter(96_000, 2, 9_600, sample);
    assert_eq!(converted.format(), native.format());
    loop {
        let expected = native.read().unwrap();
        let actual = converted.read().unwrap();
        match (expected, actual) {
            (None, None) => break,
            (Some(expected), Some(actual)) => {
                assert_eq!(expected.samples, actual.samples);
                assert_eq!(expected.position, actual.position);
            }
            _ => panic!("converted stream length differs"),
        }
    }
}

#[test]
fn high_rate_stereo_keeps_duration_channels_and_tone_frequencies() {
    let mut converted = converter(384_000, 2, 384_000, |frame, channel| {
        if channel == 0 {
            tone(384_000, 1_000., 16_384., frame)
        } else {
            tone(384_000, 10_000., 16_384., frame)
        }
    });
    assert_eq!(converted.format().sample_rate, 192_000);
    assert_eq!(converted.format().channels, 2);
    assert_eq!(converted.format().duration, Some(Duration::from_secs(1)));
    let output = drain(&mut converted);
    assert_eq!(output.frames, 192_000);
    let left = channel(&output, 2, 0);
    let right = channel(&output, 2, 1);
    let (left, right) = (&left[2_000..190_000], &right[2_000..190_000]);
    assert!((amplitude(left, 192_000, 1_000.) - 0.5).abs() < 0.01);
    assert!((amplitude(right, 192_000, 10_000.) - 0.5).abs() < 0.01);
    assert!(amplitude(left, 192_000, 10_000.) < 0.002);
    assert!(amplitude(right, 192_000, 1_000.) < 0.002);
}

#[test]
fn content_above_the_output_band_is_rejected_instead_of_folding_into_it() {
    let mut converted = converter(384_000, 1, 192_000, |frame, _| {
        tone(384_000, 150_000., 16_384., frame)
    });
    let output = drain(&mut converted);
    let folded = amplitude(&output.samples[2_000..90_000], 192_000, 42_000.);
    assert!(folded < 0.0005, "150 kHz folded to 42 kHz at {folded}");
    assert!(
        output.samples[2_000..90_000]
            .iter()
            .all(|sample| sample.abs() < 0.001)
    );
}

#[test]
fn five_point_one_is_downmixed_to_stereo_per_channel_layout() {
    let weight = 1.;
    let diagonal = std::f32::consts::FRAC_1_SQRT_2;
    for (source, expected) in [
        (0, (weight, 0.)),
        (1, (0., weight)),
        (2, (diagonal, diagonal)),
        (3, (0., 0.)),
        (4, (diagonal, 0.)),
        (5, (0., diagonal)),
    ] {
        let mut converted = converter(48_000, 6, 4_800, |_, channel| {
            if usize::from(channel) == source {
                16_384
            } else {
                0
            }
        });
        assert_eq!(converted.format().channels, 2);
        assert_eq!(converted.format().sample_rate, 48_000);
        let output = drain(&mut converted);
        assert_eq!(output.frames, 4_800);
        let frame = &output.samples[2_400 * 2..2_400 * 2 + 2];
        assert!(
            (frame[0] - expected.0 * 0.5).abs() < 1e-4
                && (frame[1] - expected.1 * 0.5).abs() < 1e-4,
            "channel {source} mixed to {frame:?}, expected {expected:?}"
        );
    }
}

#[test]
fn a_full_scale_surround_signal_cannot_clip_after_downmix() {
    let mut converted = converter(48_000, 6, 4_800, |_, _| i16::MAX);
    let peak = drain(&mut converted)
        .samples
        .iter()
        .fold(0_f32, |peak, sample| peak.max(sample.abs()));
    assert!(peak > 0.9 && peak <= 1., "peak {peak}");
}

#[test]
fn a_high_rate_surround_source_is_resampled_and_downmixed_together() {
    let mut converted = converter(384_000, 6, 96_000, |frame, channel| {
        if channel == 0 {
            tone(384_000, 2_000., 16_384., frame)
        } else {
            0
        }
    });
    assert_eq!(
        (converted.format().sample_rate, converted.format().channels),
        (192_000, 2)
    );
    let output = drain(&mut converted);
    assert_eq!(output.frames, 48_000);
    let left = channel(&output, 2, 0);
    assert!((amplitude(&left[1_000..47_000], 192_000, 2_000.) - 0.5).abs() < 0.01);
    assert!(
        channel(&output, 2, 1)[1_000..47_000]
            .iter()
            .all(|sample| sample.abs() < 1e-4)
    );
}

#[test]
fn seeking_restarts_conversion_at_the_requested_source_time() {
    let mut converted = converter(384_000, 2, 768_000, |frame, _| {
        tone(384_000, 1_000., 16_384., frame)
    });
    let first = converted.read().unwrap().unwrap().position;
    assert_eq!(first, Duration::ZERO);
    converted.seek(Duration::from_secs(1)).unwrap();
    let block = converted.read().unwrap().unwrap();
    assert!((block.position.as_secs_f64() - 1.).abs() < 1. / 192_000.);
    let mut frames = block.samples.len() / 2;
    while let Some(block) = converted.read().unwrap() {
        frames += block.samples.len() / 2;
    }
    assert_eq!(frames, 192_000);
    converted.seek(Duration::ZERO).unwrap();
    assert_eq!(drain(&mut converted).frames, 384_000);
}

#[test]
fn cancellation_reaches_the_decoder_through_conversion() {
    let mut converted = converter(384_000, 2, 38_400, |frame, _| {
        tone(384_000, 1_000., 16_384., frame)
    });
    assert!(matches!(
        converted.read_until(|| true),
        Err(Error::Interrupted)
    ));
    let mut frames = converted.read().unwrap().unwrap().samples.len() / 2;
    assert!(matches!(
        converted.read_until(|| true),
        Err(Error::Interrupted)
    ));
    while let Some(block) = converted.read().unwrap() {
        frames += block.samples.len() / 2;
    }
    assert_eq!(frames, 19_200, "interruption must not lose or repeat audio");
}

#[test]
fn only_the_standard_rate_families_are_decimated_to_a_device_rate() {
    for (rate, expected) in [
        (352_800, 176_400),
        (384_000, 192_000),
        (705_600, 176_400),
        (768_000, 192_000),
    ] {
        let source = format(rate, 2);
        assert_eq!(source.output_format().sample_rate, expected, "{rate} Hz");
        assert_eq!(source.speed_capability(), RANGE, "{rate} Hz");
    }
    for rate in [200_000, 220_500, 250_000, 192_001, 500_000, 768_001] {
        assert_eq!(
            format(rate, 2).speed_capability(),
            SpeedCapability::UnitOnly,
            "{rate} Hz"
        );
    }
}

#[test]
fn a_rate_outside_the_standard_families_is_rejected_with_a_typed_reason() {
    for rate in [200_000, 220_500, 250_000] {
        let file = support::wave(rate, 2, 1_000, |_, _| 0);
        assert!(
            matches!(Decoder::open(file), Err(Error::Unsupported)),
            "{rate} Hz"
        );
    }
}

#[test]
fn quadruple_rate_sources_decimate_by_four_and_keep_their_tones() {
    for (rate, expected) in [(705_600, 176_400), (768_000, 192_000)] {
        let mut converted = converter(rate, 2, rate / 2, |frame, _| {
            tone(rate, 3_000., 16_384., frame)
        });
        assert_eq!(converted.format().sample_rate, expected);
        let output = drain(&mut converted);
        assert_eq!(output.frames, (expected / 2) as usize);
        let left = channel(&output, 2, 0);
        let body = &left[2_000..left.len() - 2_000];
        assert!(
            (amplitude(body, expected, 3_000.) - 0.5).abs() < 0.01,
            "{rate} Hz"
        );
    }
}

#[test]
fn a_front_only_surround_tone_keeps_its_stereo_level() {
    let mut converted = converter(48_000, 6, 48_000, |frame, channel| {
        if channel < 2 {
            tone(48_000, 1_000., 16_384., frame)
        } else {
            0
        }
    });
    let output = drain(&mut converted);
    let left = channel(&output, 2, 0);
    assert!((amplitude(&left[1_000..47_000], 48_000, 1_000.) - 0.5).abs() < 0.005);
}

#[test]
fn a_loud_surround_mix_is_limited_below_full_scale_and_recovers() {
    let mut converted = converter(48_000, 8, 48_000 * 3, |frame, channel| {
        if frame < 4_800 {
            i16::MAX
        } else if channel == 0 {
            16_384
        } else {
            0
        }
    });
    let output = drain(&mut converted);
    let peak = output
        .samples
        .iter()
        .fold(0_f32, |peak, s| peak.max(s.abs()));
    assert!(peak > 0.9 && peak <= 1., "peak {peak}");
    let tail = output.samples[output.samples.len() - 2_000..]
        .iter()
        .step_by(2)
        .copied()
        .collect::<Vec<_>>();
    assert!(
        tail.iter().all(|sample| (sample - 0.5).abs() < 0.005),
        "gain did not recover: {:?}",
        &tail[..4]
    );
}
