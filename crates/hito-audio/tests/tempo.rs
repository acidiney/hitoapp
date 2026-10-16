#![cfg(feature = "device")]

#[path = "support/capture.rs"]
mod capture;
mod support;

use capture::{Capture, amplitude, wait};
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
