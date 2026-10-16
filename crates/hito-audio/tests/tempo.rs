#![cfg(feature = "device")]

#[path = "support/capture.rs"]
mod capture;
mod support;

use capture::{Capture, amplitude, energy, wait};
use hito_audio::Player;
use std::time::Duration;

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
    assert!((0.2..0.45).contains(&distance.as_secs_f64()), "slow source-time distance: {distance:?}");
    assert!(amplitude(&capture.recent(0.2), 880.) > 0.4);
    assert!(amplitude(&capture.recent(0.2), 440.) < 0.001);
    player.speed(2.).unwrap();
    let before = player.snapshot().position;
    capture.after(capture.mark(), 0.6);
    let distance = player.snapshot().position - before;
    assert!((0.8..1.4).contains(&distance.as_secs_f64()), "fast source-time distance: {distance:?}");
    player.seek(Duration::ZERO).unwrap();
    wait(|| player.snapshot().position > Duration::from_millis(400));
    capture.after(capture.mark(), 0.2);
    assert!(amplitude(&capture.recent(0.1), 440.) > 0.4);
    assert!(amplitude(&capture.recent(0.1), 880.) < 0.001);
    player.close().unwrap();
    capture.after(capture.mark(), 0.3);
    assert!(energy(&capture.recent(0.1)) < 0.000001);
}
