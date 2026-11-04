#![cfg(feature = "device")]

use hito_audio::Player;
use hito_audio::test_support as support;
use hito_audio::test_support::capture::{Capture, amplitude, energy, wait};
use std::time::Duration;

struct Forward(std::fs::File);
impl std::io::Read for Forward {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        std::io::Read::read(&mut self.0, buffer)
    }
}
impl std::io::Seek for Forward {
    fn seek(&mut self, _: std::io::SeekFrom) -> std::io::Result<u64> {
        Err(std::io::ErrorKind::Unsupported.into())
    }
}
impl hito_audio::Input for Forward {
    fn is_seekable(&self) -> bool {
        false
    }
    fn byte_len(&self) -> Option<u64> {
        None
    }
}

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

#[test]
#[ignore = "requires the isolated clocked audio monitor"]
fn sources_above_the_device_limits_play_at_unit_speed_in_converted_form() {
    for (rate, channels) in [
        (384_000, 2),
        (352_800, 2),
        (705_600, 2),
        (768_000, 2),
        (48_000, 6),
    ] {
        let capture = Capture::open();
        let mut player = Player::open(tone_clip(rate, channels, 1)).unwrap();
        assert_eq!(
            player.speed_capability(),
            hito_audio::SpeedCapability::Range { min: 0.5, max: 3. }
        );
        player.resume().unwrap();
        wait(|| player.snapshot().ended);
        let snapshot = player.snapshot();
        assert_eq!(snapshot.format.sample_rate, rate);
        assert_eq!(snapshot.format.channels, channels);
        assert!((snapshot.position.as_secs_f64() - 1.).abs() < 0.001);
        assert_eq!(snapshot.speed, 1.);
        player.close().unwrap();
        capture.after(capture.mark(), 0.2);
        let samples = capture.all();
        let active = samples.iter().filter(|sample| sample.abs() > 0.01).count();
        assert!(
            (44_000..52_000).contains(&active),
            "{rate} Hz {channels} channels emitted {active} active frames for one second"
        );
        assert!(amplitude(&samples, 440.) > amplitude(&samples, 880.) * 100.);
    }
}

#[test]
#[ignore = "requires the isolated clocked audio monitor"]
fn sources_above_the_device_limits_change_speed_with_preserved_pitch_and_duration() {
    for (rate, channels, speed, seconds, expected) in [
        (384_000, 2, 2., 4, 96_000.),
        (352_800, 2, 2., 4, 96_000.),
        (705_600, 2, 2., 4, 96_000.),
        (768_000, 2, 2., 4, 96_000.),
        (705_600, 1, 0.5, 1, 96_000.),
        (768_000, 1, 0.5, 1, 96_000.),
        (48_000, 6, 2., 4, 96_000.),
        (384_000, 1, 0.5, 1, 96_000.),
    ] {
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
        let samples = capture.all();
        let active = samples.iter().filter(|sample| sample.abs() > 0.01).count();
        assert!(
            (expected * 0.9..expected * 1.1).contains(&(active as f64)),
            "{rate} Hz {channels} channels at {speed} emitted {active} active frames, expected about {expected}"
        );
        assert!(amplitude(&samples, 440.) > amplitude(&samples, 880.) * 100.);
    }
}

#[test]
#[ignore = "requires the isolated clocked audio monitor"]
fn a_converted_source_keeps_speed_changes_seeks_and_cleanup_working() {
    let capture = Capture::open();
    let mut player = Player::open(tone_clip(384_000, 2, 12)).unwrap();
    player.resume().unwrap();
    wait(|| player.snapshot().position > Duration::from_millis(200));
    player.speed(2.).unwrap();
    wait(|| player.snapshot().speed == 2.);
    let before = player.snapshot().position;
    capture.after(capture.mark(), 0.6);
    let distance = player.snapshot().position - before;
    assert!(
        (0.8..1.4).contains(&distance.as_secs_f64()),
        "fast source-time distance: {distance:?}"
    );
    player.seek(Duration::from_secs(8)).unwrap();
    wait(|| player.snapshot().position > Duration::from_millis(8_200));
    player.speed(0.5).unwrap();
    wait(|| player.snapshot().speed == 0.5);
    let before = player.snapshot().position;
    capture.after(capture.mark(), 0.6);
    let distance = player.snapshot().position - before;
    assert!(
        (0.2..0.45).contains(&distance.as_secs_f64()),
        "slow source-time distance: {distance:?}"
    );
    player.pause();
    player.seek(Duration::ZERO).unwrap();
    player.speed(1.).unwrap();
    player.resume().unwrap();
    wait(|| player.snapshot().position > Duration::from_millis(300));
    assert!(player.snapshot().error.is_none());
    player.close().unwrap();
    capture.after(capture.mark(), 0.3);
    assert!(energy(&capture.recent(0.1)) < 0.000001);
}

#[test]
#[ignore = "requires the isolated clocked audio monitor"]
fn supported_short_clips_drain_audio_at_the_minimum_and_maximum_tempo() {
    for rate in [8_000, 16_000, 44_100, 48_000, 96_000, 192_000] {
        for speed in [0.5, 3.] {
            let capture = Capture::open();
            let input = support::wave(rate, 2, rate / 100, |frame, _| {
                ((f64::from(frame) * 440. * std::f64::consts::TAU / f64::from(rate)).sin()
                    * 16_000.) as i16
            });
            let mut player = Player::open(input).unwrap();
            player.speed(speed).unwrap();
            player.resume().unwrap();
            wait(|| player.snapshot().ended);
            assert_eq!(player.snapshot().position, Duration::from_millis(10));
            assert_eq!(player.snapshot().speed, speed);
            player.close().unwrap();
            capture.after(capture.mark(), 0.2);
            let active = capture
                .all()
                .iter()
                .filter(|sample| sample.abs() > 0.01)
                .count();
            let expected = 480. / f64::from(speed);
            assert!(
                active as f64 > expected * 0.2 && (active as f64) < expected * 1.5 + 32.,
                "{rate} Hz at {speed} emitted {active} active monitor frames, expected about {expected}"
            );
        }
    }
}

#[test]
#[ignore = "requires the isolated clocked audio monitor"]
fn short_clip_completion_retains_the_speed_of_its_presented_audio() {
    let input = support::wave(48_000, 2, 480, |frame, _| {
        ((f64::from(frame) * 440. * std::f64::consts::TAU / 48_000.).sin() * 16_000.) as i16
    });
    let mut player = Player::open(input).unwrap();
    player.speed(2.).unwrap();
    player.resume().unwrap();
    wait(|| player.snapshot().ended);
    let ended = player.snapshot();
    assert_eq!(ended.position, Duration::from_millis(10));
    assert_eq!(ended.speed, 2.);
    player.speed(3.).unwrap();
    player.seek(Duration::from_millis(10)).unwrap();
    player.resume().unwrap();
    wait(|| player.snapshot().ended);
    assert_eq!(player.snapshot().position, Duration::from_millis(10));
    assert_eq!(player.snapshot().speed, 2.);
    player.speed(0.5).unwrap();
    player.seek(Duration::ZERO).unwrap();
    player.resume().unwrap();
    wait(|| player.snapshot().ended);
    assert_eq!(player.snapshot().position, Duration::from_millis(10));
    assert_eq!(player.snapshot().speed, 0.5);
    player.close().unwrap();
}

#[test]
#[ignore = "requires the isolated clocked audio monitor"]
fn double_speed_preserves_pitch_and_reports_source_time_through_completion() {
    let capture = Capture::open();
    let input = support::wave(48_000, 2, 192_000, |frame, _| {
        ((f64::from(frame) * 440. * std::f64::consts::TAU / 48_000.).sin() * 16_000.) as i16
    });
    let mut player = Player::open(input).unwrap();
    player.speed(2.).unwrap();
    player.resume().unwrap();
    wait(|| player.snapshot().ended);
    let snapshot = player.snapshot();
    assert_eq!(snapshot.format.duration, Some(Duration::from_secs(4)));
    assert!((snapshot.position.as_secs_f64() - 4.).abs() < 0.001);
    assert_eq!(snapshot.speed, 2.);
    player.close().unwrap();
    capture.after(capture.mark(), 0.2);
    let samples = capture.all();
    let active = samples.iter().filter(|sample| sample.abs() > 0.01).count();
    assert!(
        (88_000..102_000).contains(&active),
        "active output frames: {active}"
    );
    assert!(amplitude(&samples, 440.) > amplitude(&samples, 880.) * 100.);
}

#[test]
#[ignore = "requires the isolated clocked audio monitor"]
fn speed_changes_preserve_pause_cursor_and_seek_without_stale_processed_audio() {
    let capture = Capture::open();
    let input = support::wave(48_000, 2, 480_000, |frame, _| {
        let frequency = if frame < 240_000 { 440. } else { 880. };
        ((f64::from(frame) * frequency * std::f64::consts::TAU / 48_000.).sin() * 16_000.) as i16
    });
    let mut player = Player::open(input).unwrap();
    player.resume().unwrap();
    wait(|| player.snapshot().position > Duration::from_millis(200));
    player.pause();
    capture.after(capture.mark(), 0.3);
    let paused = player.snapshot().position;
    player.speed(0.5).unwrap();
    assert!(!player.snapshot().playing);
    assert_eq!(player.snapshot().position, paused);
    capture.after(capture.mark(), 0.2);
    assert!(energy(&capture.recent(0.15)) < 0.000001);
    for invalid in [f32::NAN, f32::INFINITY, 0., 0.49, 3.01] {
        assert!(player.speed(invalid).is_err());
    }
    player.seek(Duration::from_secs(6)).unwrap();
    player.resume().unwrap();
    wait(|| player.snapshot().position > Duration::from_millis(6200));
    let before = player.snapshot().position;
    capture.after(capture.mark(), 0.6);
    let distance = player.snapshot().position - before;
    assert!(
        (0.2..0.45).contains(&distance.as_secs_f64()),
        "slow source-time distance: {distance:?}"
    );
    assert!(amplitude(&capture.recent(0.2), 880.) > 0.4);
    assert!(amplitude(&capture.recent(0.2), 440.) < 0.001);
    player.speed(2.).unwrap();
    wait(|| player.snapshot().speed == 2.);
    let before = player.snapshot().position;
    capture.after(capture.mark(), 0.6);
    let distance = player.snapshot().position - before;
    assert!(
        (0.8..1.4).contains(&distance.as_secs_f64()),
        "fast source-time distance: {distance:?}"
    );
    player.seek(Duration::ZERO).unwrap();
    wait(|| player.snapshot().position > Duration::from_millis(400));
    capture.after(capture.mark(), 0.2);
    assert!(amplitude(&capture.recent(0.1), 440.) > 0.4);
    assert!(amplitude(&capture.recent(0.1), 880.) < 0.001);
    player.close().unwrap();
    capture.after(capture.mark(), 0.3);
    assert!(energy(&capture.recent(0.1)) < 0.000001);
}

#[test]
#[ignore = "requires the isolated clocked audio monitor"]
fn a_nonseekable_stream_changes_speed_without_losing_or_replaying_audio() {
    let capture = Capture::open();
    let input = support::wave(48_000, 2, 192_000, |frame, _| {
        ((f64::from(frame) * 440. * std::f64::consts::TAU / 48_000.).sin() * 16_000.) as i16
    });
    let mut player = Player::open(Forward(input)).unwrap();
    player.resume().unwrap();
    wait(|| player.snapshot().position > Duration::from_millis(200));
    player
        .speed(2.)
        .expect("speed does not require a seekable input");
    wait(|| player.snapshot().ended);
    assert!((player.snapshot().position.as_secs_f64() - 4.).abs() < 0.001);
    player.close().unwrap();
    capture.after(capture.mark(), 0.2);
    let samples = capture.all();
    let active = samples.iter().filter(|sample| sample.abs() > 0.01).count();
    assert!(
        (93_000..126_000).contains(&active),
        "active output frames: {active}"
    );
    assert!(amplitude(&samples, 440.) > amplitude(&samples, 880.) * 100.);
}

#[test]
#[ignore = "requires the isolated clocked audio monitor"]
fn reversing_a_pending_rate_change_does_not_end_the_stream_early() {
    for rate in [48_000, 192_000] {
        let capture = Capture::open();
        let input = support::wave(rate, 2, rate * 2, |frame, _| {
            ((f64::from(frame) * 440. * std::f64::consts::TAU / f64::from(rate)).sin() * 16_000.)
                as i16
        });
        let mut player = Player::open(Forward(input)).unwrap();
        player.speed(0.5).unwrap();
        player.resume().unwrap();
        wait(|| player.snapshot().position > Duration::from_millis(200));
        player.speed(2.).unwrap();
        capture.after(capture.mark(), 0.03);
        player.speed(0.5).unwrap();
        wait(|| player.snapshot().ended);
        assert!((player.snapshot().position.as_secs_f64() - 2.).abs() < 0.001);
        assert_eq!(player.snapshot().speed, 0.5);
        player.close().unwrap();
        capture.after(capture.mark(), 0.2);
        let samples = capture.all();
        assert!(amplitude(&samples, 440.) > amplitude(&samples, 880.) * 100.);
        assert!(samples.iter().filter(|sample| sample.abs() > 0.01).count() > 48_000);
    }
}

#[test]
#[ignore = "requires the isolated clocked audio monitor"]
fn a_device_offering_only_48_khz_plays_high_rate_sources_at_every_speed() {
    for (rate, speed, seconds) in [
        (384_000, 1., 2),
        (384_000, 0.5, 1),
        (384_000, 2., 4),
        (352_800, 1., 2),
        (352_800, 2., 4),
        (705_600, 1., 2),
        (705_600, 0.5, 1),
        (705_600, 2., 4),
        (768_000, 1., 2),
        (768_000, 0.5, 1),
        (768_000, 2., 4),
        (192_000, 1., 2),
        (176_400, 0.5, 1),
        (44_100, 1., 2),
    ] {
        let capture = Capture::open();
        let mut player =
            Player::open_with_output_rates(tone_clip(rate, 2, seconds), &[48_000]).unwrap();
        assert_eq!(player.output_format().sample_rate, 48_000, "{rate} Hz");
        assert_eq!(player.output_format().channels, 2);
        assert_eq!(
            player.speed_capability(),
            hito_audio::SpeedCapability::Range { min: 0.5, max: 3. }
        );
        if speed != 1. {
            player.speed(speed).unwrap();
        }
        player.resume().unwrap();
        wait(|| player.snapshot().ended);
        let snapshot = player.snapshot();
        assert!((snapshot.position.as_secs_f64() - f64::from(seconds)).abs() < 0.001);
        assert_eq!(snapshot.format.sample_rate, rate);
        player.close().unwrap();
        capture.after(capture.mark(), 0.2);
        let samples = capture.all();
        let active = samples.iter().filter(|sample| sample.abs() > 0.01).count();
        let expected = f64::from(seconds) * 48_000. / f64::from(speed);
        assert!(
            (expected * 0.9..expected * 1.1).contains(&(active as f64)),
            "{rate} Hz at {speed} emitted {active} active frames, expected about {expected}"
        );
        assert!(
            amplitude(&samples, 440.) > amplitude(&samples, 880.) * 100.,
            "{rate} Hz at {speed} lost its 440 Hz tone: {} vs {}",
            amplitude(&samples, 440.),
            amplitude(&samples, 880.)
        );
    }
}

#[test]
#[ignore = "requires the isolated clocked audio monitor"]
fn a_device_with_no_usable_rate_fails_with_a_plain_typed_error() {
    let error = Player::open_with_output_rates(tone_clip(48_000, 2, 1), &[])
        .err()
        .unwrap();
    assert_eq!(error, hito_audio::Error::OutputFormat);
    assert_eq!(
        error.to_string(),
        "This audio device can't play this file's format"
    );
}
