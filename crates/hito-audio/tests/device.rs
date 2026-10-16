#![cfg(feature = "device")]

#[path = "support/capture.rs"]
mod capture;
mod support;

use capture::{Capture, amplitude, energy, wait};
use hito_audio::Player;
use std::time::Duration;

#[test]
#[ignore = "requires the isolated clocked PulseAudio monitor in scripts/check-audio.sh"]
fn real_device_output_contains_the_decoded_tone_and_finishes() {
    let capture = Capture::open();
    let input = support::wave(48_000, 2, 48_000, |frame, _| {
        ((f64::from(frame) * 440. * std::f64::consts::TAU / 48_000.).sin() * 16_000.) as i16
    });
    let mut player = Player::open(input).unwrap();
    player.resume().unwrap();
    wait(|| player.snapshot().ended);
    assert!((player.snapshot().position.as_secs_f64() - 1.).abs() < 0.001);
    player.close().unwrap();
    capture.after(capture.mark(), 0.2);
    let samples = capture.all();
    assert!(amplitude(&samples, 440.) > amplitude(&samples, 880.) * 100.);
    let active: Vec<f32> = samples
        .into_iter()
        .filter(|sample| sample.abs() > 0.01)
        .collect();
    assert!(
        active.len() > 35_000,
        "the device monitor did not capture audio"
    );
    let energy = energy(&active);
    assert!(
        (0.1..0.15).contains(&energy),
        "unexpected device signal energy {energy}"
    );
}

#[test]
#[ignore = "requires the isolated clocked PulseAudio monitor in scripts/check-audio.sh"]
fn pause_and_seek_control_the_emitted_audio_without_stale_buffered_samples() {
    let capture = Capture::open();
    let input = support::wave(48_000, 2, 480_000, |frame, _| {
        let frequency = if frame < 240_000 { 440. } else { 880. };
        ((f64::from(frame) * frequency * std::f64::consts::TAU / 48_000.).sin() * 16_000.) as i16
    });
    let mut player = Player::open(input).unwrap();
    player.resume().unwrap();
    wait(|| player.snapshot().position > Duration::from_millis(200));
    player.pause();
    capture.after(capture.mark(), 0.25);
    let paused = player.snapshot().position;
    capture.after(capture.mark(), 0.2);
    assert_eq!(player.snapshot().position, paused);
    assert!(energy(&capture.recent(0.15)) < 0.000001);
    player.seek(Duration::from_secs(6)).unwrap();
    assert!(!player.snapshot().playing);
    assert_eq!(player.snapshot().position, Duration::from_secs(6));
    player.resume().unwrap();
    wait(|| player.snapshot().position > Duration::from_millis(6200));
    capture.after(capture.mark(), 0.2);
    assert!(amplitude(&capture.recent(0.1), 880.) > 0.4);
    assert!(amplitude(&capture.recent(0.1), 440.) < 0.001);
    player.seek(Duration::ZERO).unwrap();
    wait(|| player.snapshot().position > Duration::from_millis(200));
    assert!(player.snapshot().position < Duration::from_secs(2));
    capture.after(capture.mark(), 0.2);
    assert!(amplitude(&capture.recent(0.1), 440.) > 0.4);
    assert!(amplitude(&capture.recent(0.1), 880.) < 0.001);
    player.close().unwrap();
    capture.after(capture.mark(), 0.3);
    assert!(energy(&capture.recent(0.1)) < 0.000001);
}

#[test]
#[ignore = "requires the isolated clocked PulseAudio monitor in scripts/check-audio.sh"]
fn volume_changes_actual_output_and_invalid_values_do_not_replace_it() {
    let capture = Capture::open();
    let input = support::wave(48_000, 2, 240_000, |frame, _| {
        ((f64::from(frame) * 440. * std::f64::consts::TAU / 48_000.).sin() * 16_000.) as i16
    });
    let mut player = Player::open(input).unwrap();
    player.volume(0.5).unwrap();
    player.resume().unwrap();
    wait(|| player.snapshot().position > Duration::from_millis(200));
    capture.after(capture.mark(), 0.2);
    assert!((0.23..0.26).contains(&amplitude(&capture.recent(0.1), 440.)));
    for invalid in [f32::NAN, f32::INFINITY, -0.1, 1.1] {
        assert!(player.volume(invalid).is_err());
    }
    capture.after(capture.mark(), 0.2);
    assert!((0.23..0.26).contains(&amplitude(&capture.recent(0.1), 440.)));
    player.volume(0.).unwrap();
    capture.after(capture.mark(), 0.3);
    assert!(energy(&capture.recent(0.1)) < 0.000001);
    drop(player);
}

#[test]
#[ignore = "requires the isolated clocked PulseAudio monitor in scripts/check-audio.sh"]
fn a_revocation_owner_can_stop_output_without_acquiring_the_player() {
    let capture = Capture::open();
    let mut player = Player::open(support::wave(48_000, 2, 240_000, |frame, _| {
        ((f64::from(frame) * 440. * std::f64::consts::TAU / 48_000.).sin() * 16_000.) as i16
    }))
    .unwrap();
    player.resume().unwrap();
    wait(|| player.snapshot().position > Duration::from_millis(200));
    let stop = player.stop_handle();
    std::thread::spawn(move || stop.request()).join().unwrap();
    assert!(!player.snapshot().playing);
    assert!(matches!(
        player.resume(),
        Err(hito_audio::Error::Interrupted)
    ));
    assert!(matches!(
        player.seek(Duration::ZERO),
        Err(hito_audio::Error::Interrupted)
    ));
    capture.after(capture.mark(), 0.3);
    assert!(energy(&capture.recent(0.1)) < 0.000001);
    player.close().unwrap();
}

#[test]
#[cfg(target_os = "linux")]
#[ignore = "requires the isolated clocked PulseAudio monitor in scripts/check-audio.sh"]
fn closing_or_dropping_joins_the_worker_and_releases_its_owned_input() {
    use std::os::unix::fs::MetadataExt;
    for explicit in [true, false] {
        let input = support::wave(48_000, 2, 480_000, |_, _| 1);
        let identity = input.metadata().unwrap();
        let mut player = Player::open(input).unwrap();
        player.resume().unwrap();
        wait(|| player.snapshot().position > Duration::from_millis(20));
        if explicit {
            player.close().unwrap();
        } else {
            drop(player);
        }
        assert!(
            !std::fs::read_dir("/proc/self/fd")
                .unwrap()
                .flatten()
                .any(|entry| {
                    std::fs::metadata(entry.path()).is_ok_and(|metadata| {
                        metadata.dev() == identity.dev() && metadata.ino() == identity.ino()
                    })
                }),
            "owned decoder input survived shutdown"
        );
        wait(|| {
            !std::fs::read_dir("/proc/self/task")
                .unwrap()
                .flatten()
                .any(|entry| {
                    std::fs::read_to_string(entry.path().join("comm"))
                        .is_ok_and(|name| name.trim() == "hito-audio")
                })
        });
    }
}

#[test]
#[ignore = "requires the isolated clocked PulseAudio monitor in scripts/check-audio.sh"]
fn end_seeks_and_repeated_paused_seeks_can_restart_from_zero() {
    let mut player = Player::open(support::wave(48_000, 2, 24_000, |_, _| 1)).unwrap();
    player.resume().unwrap();
    wait(|| player.snapshot().ended);
    assert!(!player.snapshot().playing);
    for target in [0, 300, 50, 500, 0] {
        player.seek(Duration::from_millis(target)).unwrap();
        assert!(!player.snapshot().playing);
        assert_eq!(player.snapshot().position, Duration::from_millis(target));
    }
    player.resume().unwrap();
    wait(|| player.snapshot().ended);
    assert_eq!(player.snapshot().position, Duration::from_millis(500));
    player.close().unwrap();
}

#[test]
#[ignore = "requires the isolated clocked PulseAudio monitor in scripts/check-audio.sh"]
fn completion_preserves_the_final_marker_when_the_player_is_closed_immediately() {
    let capture = Capture::open();
    let input = support::wave(48_000, 2, 2400, |frame, _| {
        let frequency = if frame < 1200 { 440. } else { 1760. };
        ((f64::from(frame) * frequency * std::f64::consts::TAU / 48_000.).sin() * 16_000.) as i16
    });
    let mut player = Player::open(input).unwrap();
    player.resume().unwrap();
    wait(|| player.snapshot().ended);
    let final_state = player.snapshot();
    player.close().unwrap();
    capture.after(capture.mark(), 0.3);
    let samples = capture.all();
    let high = amplitude(&samples, 1760.);
    assert!(high > 0.005, "final tone was lost: amplitude {high}");
    let active = samples.iter().filter(|sample| sample.abs() > 0.01).count();
    assert!(
        active >= 2300,
        "only {active} active frames survived a 2400-frame clip, xruns={}, underruns={}, low={}",
        final_state.xruns,
        final_state.underruns,
        amplitude(&samples, 440.)
    );
}

#[test]
#[cfg(target_os = "linux")]
#[ignore = "requires the isolated clocked PulseAudio monitor in scripts/check-audio.sh"]
fn an_actual_device_underrun_recovers_at_the_presented_position() {
    use std::process::Command;
    let capture = Capture::open();
    let mut player = Player::open(support::wave(48_000, 2, 240_000, |frame, _| {
        ((f64::from(frame) * 440. * std::f64::consts::TAU / 48_000.).sin() * 16_000.) as i16
    }))
    .unwrap();
    player.resume().unwrap();
    wait(|| player.snapshot().position > Duration::from_millis(200));
    let position = player.snapshot().position;
    let pid = std::process::id().to_string();
    let status = std::fs::read_to_string("/proc/self/status").unwrap();
    let parent = status
        .lines()
        .find_map(|line| line.strip_prefix("PPid:"))
        .unwrap()
        .trim();
    assert_eq!(
        std::fs::read_to_string(format!("/proc/{parent}/comm"))
            .unwrap()
            .trim(),
        "runuser"
    );
    let mut resume = Command::new("sh")
        .args([
            "-c",
            "sleep 0.4; kill -CONT \"$1\" \"$2\"",
            "sh",
            &pid,
            parent,
        ])
        .spawn()
        .unwrap();
    assert!(
        Command::new("kill")
            .args(["-STOP", &pid])
            .status()
            .unwrap()
            .success()
    );
    assert!(resume.wait().unwrap().success());
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while player.snapshot().position <= position + Duration::from_millis(100) {
        assert!(
            std::time::Instant::now() < deadline,
            "output did not recover: position={:?}, xruns={}, error={:?}, output_failure={:?}, playing={}, ended={}",
            player.snapshot().position,
            player.snapshot().xruns,
            player.snapshot().error,
            player.snapshot().output_failure,
            player.snapshot().playing,
            player.snapshot().ended
        );
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(
        player.snapshot().xruns > 0,
        "the device did not report an underrun, position={:?}, error={:?}",
        player.snapshot().position,
        player.snapshot().error
    );
    assert!(player.snapshot().error.is_none());
    capture.after(capture.mark(), 0.2);
    assert!(amplitude(&capture.recent(0.1), 440.) > 0.4);
    player.close().unwrap();
}
