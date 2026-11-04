#![cfg(feature = "device")]

use hito_audio::Player;
use hito_audio::test_support as support;
use hito_audio::test_support::capture::{Capture, amplitude, wait};

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

#[test]
#[ignore = "requires the isolated clocked audio monitor"]
fn a_device_offering_only_48_khz_plays_quadruple_rate_sources_at_unit_half_and_double_speed() {
    for rate in RATES {
        for (speed, seconds) in [(1., 2), (0.5, 1), (2., 4)] {
            let capture = Capture::open();
            let mut player =
                Player::open_with_output_rates(tone_clip(rate, 2, seconds), &[48_000]).unwrap();
            assert_eq!(player.output_format().sample_rate, 48_000, "{rate} Hz");
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
            assert_tone(
                &capture.all(),
                f64::from(seconds) * 48_000. / f64::from(speed),
                &format!("{rate} Hz at {speed}"),
            );
        }
    }
}
