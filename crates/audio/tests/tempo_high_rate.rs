#![cfg(feature = "device")]

use hito_audio::Player;
use hito_audio::test_support as support;
use hito_audio::test_support::capture::{Capture, amplitude, wait};
use hito_audio::test_support::output_witness::OutputWitness;
use std::collections::BTreeSet;
use tracing_subscriber::{filter::filter_fn, prelude::*};

const RATES: [u32; 2] = [705_600, 768_000];

fn tone_clip(rate: u32, channels: u16, seconds: u32) -> std::fs::File {
    support::wave(rate, channels, rate * seconds, |frame, channel| {
        if channel == 0 {
            ((f64::from(frame) * 440. * std::f64::consts::TAU / f64::from(rate)).sin() * 16_000.)
                as i16
        } else {
            0
        }
    })
}

fn assert_tone(samples: &[f32], expected: f64, label: &str) {
    let active = samples.iter().filter(|sample| sample.abs() > 0.01).count();
    assert!(
        (expected * 0.9..expected * 1.1).contains(&(active as f64)),
        "{label} emitted {active} active frames, expected about {expected}"
    );
    assert!(
        amplitude(samples, 440.) > amplitude(samples, 880.) * 100.,
        "{label} lost its 440 Hz tone"
    );
}

#[test]
#[ignore = "requires the isolated clocked audio monitor"]
fn quadruple_rate_sources_play_at_unit_speed_in_converted_form() {
    for rate in RATES {
        let capture = Capture::open();
        let mut player = Player::open(tone_clip(rate, 2, 1)).unwrap();
        assert_eq!(
            player.speed_capability(),
            hito_audio::SpeedCapability::Range { min: 0.5, max: 3. }
        );
        player.resume().unwrap();
        wait(|| player.snapshot().ended);
        let snapshot = player.snapshot();
        assert_eq!(snapshot.format.sample_rate, rate);
        assert_eq!(snapshot.speed, 1.);
        assert!((snapshot.position.as_secs_f64() - 1.).abs() < 0.001);
        player.close().unwrap();
        capture.after(capture.mark(), 0.2);
        assert_tone(&capture.all(), 48_000., &format!("{rate} Hz"));
    }
}

#[test]
#[ignore = "requires the isolated clocked audio monitor"]
fn quadruple_rate_sources_change_speed_with_preserved_pitch_and_duration() {
    for rate in RATES {
        for (channels, speed, seconds) in [(1, 2., 4), (1, 0.5, 1)] {
            let capture = Capture::open();
            let mut player = Player::open(tone_clip(rate, channels, seconds)).unwrap();
            player.speed(speed).unwrap();
            player.resume().unwrap();
            wait(|| player.snapshot().ended);
            let snapshot = player.snapshot();
            assert!((snapshot.position.as_secs_f64() - f64::from(seconds)).abs() < 0.001);
            assert_eq!(snapshot.speed, speed);
            player.close().unwrap();
            capture.after(capture.mark(), 0.2);
            assert_tone(
                &capture.all(),
                96_000.,
                &format!("{rate} Hz {channels} channels at {speed}"),
            );
        }
    }
}

#[derive(Default)]
struct HighRatePitchSampleSummary {
    frames: usize,
    active: usize,
    nonfinite: usize,
    mean_square: f64,
    amplitude_440: f64,
    amplitude_880: f64,
    energy_440: f64,
    energy_880: f64,
    ratio_440_880: Option<f64>,
    windows: HighRatePitchWindowSummary,
}

#[derive(Default)]
struct HighRatePitchWindowSummary {
    count: usize,
    analyzed: usize,
    overflow: bool,
    min_440_energy: Option<f64>,
    max_440_energy: Option<f64>,
    min_880_energy: Option<f64>,
    max_880_energy: Option<f64>,
    dominant_440: usize,
    dominant_880: usize,
    zero_880_energy: usize,
    lowest_ratio: Option<(usize, f64, f64, f64)>,
}

fn high_rate_pitch_sample_summary(samples: &[f32]) -> HighRatePitchSampleSummary {
    let active = samples.iter().filter(|sample| sample.abs() > 0.01).count();
    let nonfinite = samples.iter().filter(|sample| !sample.is_finite()).count();
    let mean_square = if samples.is_empty() {
        0.
    } else {
        samples
            .iter()
            .map(|sample| f64::from(*sample).powi(2))
            .sum::<f64>()
            / samples.len() as f64
    };
    let amplitude_440 = amplitude(samples, 440.);
    let amplitude_880 = amplitude(samples, 880.);
    let energy_440 = amplitude_440.powi(2);
    let energy_880 = amplitude_880.powi(2);
    let ratio = if energy_880 == 0. {
        None
    } else {
        Some(energy_440 / energy_880)
    };
    let mut windows = HighRatePitchWindowSummary::default();
    windows.count = samples.len().div_ceil(4800);
    windows.overflow = windows.count > 300;
    for (index, window) in samples.chunks(4800).take(300).enumerate() {
        windows.analyzed += 1;
        let window_440 = amplitude(window, 440.).powi(2);
        let window_880 = amplitude(window, 880.).powi(2);
        windows.min_440_energy = Some(
            windows
                .min_440_energy
                .map_or(window_440, |v| v.min(window_440)),
        );
        windows.max_440_energy = Some(
            windows
                .max_440_energy
                .map_or(window_440, |v| v.max(window_440)),
        );
        windows.min_880_energy = Some(
            windows
                .min_880_energy
                .map_or(window_880, |v| v.min(window_880)),
        );
        windows.max_880_energy = Some(
            windows
                .max_880_energy
                .map_or(window_880, |v| v.max(window_880)),
        );
        windows.dominant_440 += usize::from(window_440 > window_880);
        windows.dominant_880 += usize::from(window_880 > window_440);
        if window_880 == 0. {
            windows.zero_880_energy += 1;
        } else {
            let window_ratio = window_440 / window_880;
            if window_ratio.is_finite()
                && windows
                    .lowest_ratio
                    .is_none_or(|(_, current, _, _)| window_ratio < current)
            {
                windows.lowest_ratio = Some((index, window_ratio, window_440, window_880));
            }
        }
    }
    HighRatePitchSampleSummary {
        frames: samples.len(),
        active,
        nonfinite,
        mean_square,
        amplitude_440,
        amplitude_880,
        energy_440,
        energy_880,
        ratio_440_880: ratio,
        windows,
    }
}

fn print_high_rate_pitch_sample_summary(label: &str, samples: &[f32]) {
    let summary = high_rate_pitch_sample_summary(samples);
    println!(
        "pitch_diag samples={label} frames={} active={} nonfinite={} mean_square={:.8e} amp440={:.8e} amp880={:.8e} energy440={:.8e} energy880={:.8e} ratio440_880={:?} windows={} analyzed={} overflow={} min440={:?} max440={:?} min880={:?} max880={:?} dominant440={} dominant880={} zero880={} lowest_ratio={:?}",
        summary.frames,
        summary.active,
        summary.nonfinite,
        summary.mean_square,
        summary.amplitude_440,
        summary.amplitude_880,
        summary.energy_440,
        summary.energy_880,
        summary.ratio_440_880,
        summary.windows.count,
        summary.windows.analyzed,
        summary.windows.overflow,
        summary.windows.min_440_energy,
        summary.windows.max_440_energy,
        summary.windows.min_880_energy,
        summary.windows.max_880_energy,
        summary.windows.dominant_440,
        summary.windows.dominant_880,
        summary.windows.zero_880_energy,
        summary.windows.lowest_ratio,
    );
}

fn print_high_rate_pitch_callback_summary(callbacks: &[support::output_witness::Callback]) {
    let owners: BTreeSet<_> = callbacks.iter().map(|callback| callback.owner).collect();
    let generations: BTreeSet<_> = callbacks
        .iter()
        .map(|callback| callback.generation)
        .collect();
    let rates: BTreeSet<_> = callbacks
        .iter()
        .map(|callback| callback.sample_rate)
        .collect();
    let channels: BTreeSet<_> = callbacks.iter().map(|callback| callback.channels).collect();
    let generation_changes: Vec<_> = callbacks
        .iter()
        .enumerate()
        .filter_map(|(index, callback)| {
            (index == 0 || callbacks[index - 1].generation != callback.generation).then_some((
                index,
                callback.generation,
                callback.offset,
            ))
        })
        .take(129)
        .collect();
    let generation_changes_overflow = generation_changes.len() > 128;
    let rates_reported: Vec<_> = rates.iter().take(32).copied().collect();
    let channels_reported: Vec<_> = channels.iter().take(32).copied().collect();
    let max_xruns = callbacks
        .iter()
        .map(|callback| callback.xruns)
        .max()
        .unwrap_or(0);
    let zero_frames: usize = callbacks.iter().map(|callback| callback.zeroes).sum();
    println!(
        "pitch_diag callbacks count={} owner_count={} generation_count={} generation_changes={:?} generation_changes_overflow={} rate_count={} rates={rates_reported:?} rate_values_overflow={} channel_count={} channels={channels_reported:?} channel_values_overflow={} max_xruns={max_xruns} zero_frames={zero_frames} first_callback_ns={:?} last_callback_ns={:?}",
        callbacks.len(),
        owners.len(),
        generations.len(),
        &generation_changes[..generation_changes.len().min(128)],
        generation_changes_overflow,
        rates.len(),
        rates.len() > rates_reported.len(),
        channels.len(),
        channels.len() > channels_reported.len(),
        callbacks.first().map(|callback| callback.callback_ns),
        callbacks.last().map(|callback| callback.callback_ns),
    );
}

#[test]
#[ignore = "requires the isolated clocked audio monitor"]
fn a_device_offering_only_48_khz_plays_quadruple_rate_sources_at_unit_half_and_double_speed() {
    let subscriber = tracing_subscriber::registry().with(
        tracing_subscriber::fmt::layer()
            .with_target(false)
            .with_filter(filter_fn(|metadata| {
                metadata.target() == "hito_audio::producer_diagnostic"
            })),
    );
    let _subscriber_guard = tracing::subscriber::set_default(subscriber);
    for rate in RATES {
        for (speed, seconds) in [(1., 2), (0.5, 1), (2., 4)] {
            let witness = OutputWitness::begin(30 * 48_000, 8192).unwrap();
            let capture = Capture::open();
            let mut player =
                Player::open_with_output_rates(tone_clip(rate, 2, seconds), &[48_000]).unwrap();
            let output = player.output_format();
            if speed != 1. {
                player.speed(speed).unwrap();
            }
            player.resume().unwrap();
            wait(|| player.snapshot().ended);
            let snapshot = player.snapshot();
            println!(
                "pitch_diag tuple rate={rate} speed={speed} seconds={seconds} snapshot playing={} buffer_ready={} ended={} position_s={:.6} speed={} source_rate={} source_channels={} output_rate={} output_channels={} error={:?} output_failure={:?} underruns={} xruns={} notices={}",
                snapshot.playing,
                snapshot.buffer_ready,
                snapshot.ended,
                snapshot.position.as_secs_f64(),
                snapshot.speed,
                snapshot.format.sample_rate,
                snapshot.format.channels,
                output.sample_rate,
                output.channels,
                snapshot.error,
                snapshot.output_failure,
                snapshot.underruns,
                snapshot.xruns,
                snapshot.notices,
            );
            assert_eq!(output.sample_rate, 48_000, "{rate} Hz");
            assert!((snapshot.position.as_secs_f64() - f64::from(seconds)).abs() < 0.001);
            assert_eq!(snapshot.format.sample_rate, rate);
            player.close().unwrap();
            let report = witness.finish().unwrap_or_else(|error| {
                panic!("{rate} Hz at {speed}: output witness incomplete: {error:?}")
            });
            capture.after(capture.mark(), 0.2);
            let monitored = capture.all();
            print_high_rate_pitch_callback_summary(&report.callbacks);
            let owners: BTreeSet<_> = report
                .callbacks
                .iter()
                .map(|callback| callback.owner)
                .collect();
            assert_eq!(
                owners.len(),
                1,
                "{rate} Hz at {speed}: output witness owner changed"
            );
            assert!(
                report
                    .callbacks
                    .iter()
                    .all(|callback| callback.sample_rate == 48_000),
                "{rate} Hz at {speed}: output witness rate changed"
            );
            print_high_rate_pitch_sample_summary("callback", &report.samples);
            print_high_rate_pitch_sample_summary("monitor", &monitored);
            assert_tone(
                &monitored,
                f64::from(seconds) * 48_000. / f64::from(speed),
                &format!("{rate} Hz at {speed}"),
            );
        }
    }
}
