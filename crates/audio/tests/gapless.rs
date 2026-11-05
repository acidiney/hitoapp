#![cfg(feature = "device")]

use hito_audio::Player;
use hito_audio::test_support as support;
use hito_audio::test_support::capture::{Capture, wait};
use hito_audio::test_support::continuity::{audit, counter, dump, heard, level, longest_run};
use std::{fs::File, path::Path, process::Command, time::Duration};

const RATE: u32 = 48_000;
const HALF_SECOND: u32 = 24_000;

fn phase_sine(frame: u64) -> i16 {
    ((frame as f64 * 440. * std::f64::consts::TAU / f64::from(RATE) + 0.3).sin() * 16_000.) as i16
}

fn wave(rate: u32, start: u64, frames: u32, sample: fn(u64) -> i16) -> File {
    support::wave(rate, 2, frames, move |frame, _| {
        sample(start + u64::from(frame))
    })
}

fn encode(source: File, path: &Path, codec: &str) {
    let wav = path.with_extension("wav");
    std::io::copy(
        &mut std::io::BufReader::new(source),
        &mut File::create(&wav).unwrap(),
    )
    .unwrap();
    let result = Command::new("ffmpeg")
        .args(["-nostdin", "-hide_banner", "-loglevel", "error", "-i"])
        .arg(&wav)
        .args(["-c:a", codec, "-y"])
        .arg(path)
        .output()
        .unwrap();
    assert!(result.status.success(), "fixture encoder {codec} failed");
}

fn finish(capture: &Capture, player: Player) -> Vec<f32> {
    player.close().unwrap();
    capture.after(capture.mark(), 0.3);
    capture.all()
}

#[test]
#[ignore = "requires the isolated clocked PulseAudio monitor in scripts/check-audio.sh"]
fn reopening_the_player_between_tracks_leaves_an_audible_gap() {
    let capture = Capture::open();
    let mut first = Player::open(wave(RATE, 0, HALF_SECOND, counter)).unwrap();
    first.resume().unwrap();
    wait(|| first.snapshot().ended);
    first.close().unwrap();
    let mut second =
        Player::open(wave(RATE, u64::from(HALF_SECOND), HALF_SECOND, counter)).unwrap();
    second.resume().unwrap();
    wait(|| second.snapshot().ended);
    let captured = finish(&capture, second);
    dump("engine-reopen", &captured);
    let audit = audit(&captured, counter, 2 * HALF_SECOND as usize);
    eprintln!("reopen gap frames={}", audit.longest_silence);
    assert!(audit.longest_silence > 0);
}

#[test]
#[ignore = "requires the isolated clocked PulseAudio monitor in scripts/check-audio.sh"]
fn wav_to_wav_inserts_no_silence_and_keeps_every_sample() {
    let capture = Capture::open();
    let mut player = Player::open(wave(RATE, 0, HALF_SECOND, counter)).unwrap();
    player
        .prepare_next(wave(RATE, u64::from(HALF_SECOND), HALF_SECOND, counter))
        .unwrap();
    player.resume().unwrap();
    wait(|| player.snapshot().ended);
    assert_eq!(player.snapshot().track, 1);
    assert_eq!(player.snapshot().position, Duration::from_millis(500));
    let captured = finish(&capture, player);
    dump("engine-gapless", &captured);
    let audit = audit(&captured, counter, 2 * HALF_SECOND as usize);
    eprintln!("wav gap frames={}", audit.longest_silence);
    assert_eq!(audit.longest_silence, 0);
    assert_eq!(audit.mismatches, 0);
    assert_eq!(audit.heard, 2 * HALF_SECOND as usize);
}

#[test]
#[ignore = "requires the isolated clocked PulseAudio monitor in scripts/check-audio.sh"]
fn flac_to_flac_inserts_no_silence_and_keeps_every_sample() {
    let root = tempfile::tempdir().unwrap();
    let (first, second) = (root.path().join("a.flac"), root.path().join("b.flac"));
    encode(wave(RATE, 0, HALF_SECOND, counter), &first, "flac");
    encode(
        wave(RATE, u64::from(HALF_SECOND), HALF_SECOND, counter),
        &second,
        "flac",
    );
    let capture = Capture::open();
    let mut player = Player::open(File::open(&first).unwrap()).unwrap();
    player.prepare_next(File::open(&second).unwrap()).unwrap();
    player.resume().unwrap();
    wait(|| player.snapshot().ended);
    let captured = finish(&capture, player);
    let audit = audit(&captured, counter, 2 * HALF_SECOND as usize);
    eprintln!("flac gap frames={}", audit.longest_silence);
    assert_eq!(audit.longest_silence, 0);
    assert_eq!(audit.mismatches, 0);
    assert_eq!(audit.heard, 2 * HALF_SECOND as usize);
}

#[test]
#[ignore = "requires the isolated clocked PulseAudio monitor in scripts/check-audio.sh"]
fn a_sine_keeps_its_phase_across_the_boundary() {
    let capture = Capture::open();
    let mut player = Player::open(wave(RATE, 0, 20_000, phase_sine)).unwrap();
    player
        .prepare_next(wave(RATE, 20_000, 28_000, phase_sine))
        .unwrap();
    player.resume().unwrap();
    wait(|| player.snapshot().ended);
    let captured = finish(&capture, player);
    let audit = audit(&captured, phase_sine, 48_000);
    assert_eq!(audit.mismatches, 0);
    assert_eq!(audit.heard, 48_000);
}

#[test]
#[ignore = "requires the isolated clocked PulseAudio monitor in scripts/check-audio.sh"]
fn a_rate_mismatch_is_resampled_into_the_same_stream_with_a_bounded_gap() {
    let capture = Capture::open();
    let mut player = Player::open(wave(RATE, 0, HALF_SECOND, counter)).unwrap();
    player
        .prepare_next(wave(44_100, 0, 22_050, counter))
        .unwrap();
    player.resume().unwrap();
    wait(|| player.snapshot().ended);
    assert_eq!(player.snapshot().track, 1);
    let captured = finish(&capture, player);
    let audit = audit(&captured, counter, HALF_SECOND as usize);
    eprintln!(
        "44.1 to 48 kHz gap frames={} heard={}",
        audit.longest_silence, audit.heard
    );
    assert!(audit.longest_silence <= 48);
    assert!(
        (47_900..=48_100).contains(&audit.heard),
        "unexpected length {}",
        audit.heard
    );
}

#[test]
#[ignore = "requires the isolated clocked PulseAudio monitor in scripts/check-audio.sh"]
fn clearing_the_next_input_ends_playback_at_the_first_track() {
    let capture = Capture::open();
    let mut player = Player::open(wave(RATE, 0, HALF_SECOND, counter)).unwrap();
    player
        .prepare_next(wave(RATE, u64::from(HALF_SECOND), HALF_SECOND, counter))
        .unwrap();
    assert!(player.cancel_next().unwrap());
    assert!(!player.cancel_next().unwrap());
    player.resume().unwrap();
    wait(|| player.snapshot().ended);
    assert_eq!(player.snapshot().track, 0);
    let captured = finish(&capture, player);
    assert_eq!(heard(&captured).len(), HALF_SECOND as usize);
}

#[test]
#[ignore = "requires the isolated clocked PulseAudio monitor in scripts/check-audio.sh"]
fn replacing_the_next_input_plays_only_the_latest() {
    let capture = Capture::open();
    let mut player = Player::open(wave(RATE, 0, HALF_SECOND, counter)).unwrap();
    player.prepare_next(wave(RATE, 7, 6_000, counter)).unwrap();
    player
        .prepare_next(wave(RATE, u64::from(HALF_SECOND), HALF_SECOND, counter))
        .unwrap();
    player.resume().unwrap();
    wait(|| player.snapshot().ended);
    let captured = finish(&capture, player);
    let audit = audit(&captured, counter, 2 * HALF_SECOND as usize);
    assert_eq!(audit.mismatches, 0);
    assert_eq!(audit.heard, 2 * HALF_SECOND as usize);
}

#[test]
#[ignore = "requires the isolated clocked PulseAudio monitor in scripts/check-audio.sh"]
fn a_next_input_prepared_after_the_first_track_ended_is_refused() {
    let mut player = Player::open(wave(RATE, 0, 2_400, counter)).unwrap();
    player.resume().unwrap();
    wait(|| player.snapshot().ended);
    assert!(matches!(
        player.prepare_next(wave(RATE, 2_400, 2_400, counter)),
        Err(hito_audio::Error::Interrupted)
    ));
    assert_eq!(player.snapshot().track, 0);
    player.close().unwrap();
}

#[test]
#[ignore = "requires the isolated clocked PulseAudio monitor in scripts/check-audio.sh"]
fn seeking_near_the_end_and_after_the_boundary_keeps_the_prepared_track() {
    let capture = Capture::open();
    let mut player = Player::open(wave(RATE, 0, HALF_SECOND, counter)).unwrap();
    player
        .prepare_next(wave(RATE, u64::from(HALF_SECOND), HALF_SECOND, counter))
        .unwrap();
    player.seek(Duration::from_millis(400)).unwrap();
    player.resume().unwrap();
    wait(|| player.snapshot().track == 1);
    wait(|| player.snapshot().position > Duration::from_millis(100));
    player.seek(Duration::from_millis(250)).unwrap();
    player.resume().unwrap();
    wait(|| player.snapshot().ended);
    assert_eq!(player.snapshot().track, 1);
    let captured = finish(&capture, player);
    let heard = heard(&captured);
    assert!(heard.len() >= 4_800 + 4_800 + 12_000);
    assert_eq!(
        level(heard[4_799]),
        i32::from(counter(u64::from(HALF_SECOND) - 1))
    );
    assert_eq!(
        level(heard[4_800]),
        i32::from(counter(u64::from(HALF_SECOND)))
    );
}

#[test]
#[ignore = "requires the isolated clocked PulseAudio monitor in scripts/check-audio.sh"]
fn pausing_across_the_boundary_resumes_without_dropping_or_repeating_samples() {
    let capture = Capture::open();
    let mut player = Player::open(wave(RATE, 0, 2 * HALF_SECOND, counter)).unwrap();
    player
        .prepare_next(wave(
            RATE,
            u64::from(2 * HALF_SECOND),
            2 * HALF_SECOND,
            counter,
        ))
        .unwrap();
    player.resume().unwrap();
    wait(|| player.snapshot().position > Duration::from_millis(900));
    player.pause();
    let paused_track = player.snapshot().track;
    capture.after(capture.mark(), 0.4);
    assert_eq!(player.snapshot().track, paused_track);
    player.resume().unwrap();
    wait(|| player.snapshot().ended);
    assert_eq!(player.snapshot().track, 1);
    let captured = finish(&capture, player);
    let heard = heard(&captured);
    assert_eq!(heard.len(), 4 * HALF_SECOND as usize);
    for (index, sample) in heard.iter().enumerate() {
        assert!(
            (level(*sample) - i32::from(counter(index as u64))).abs() <= 1,
            "sample {index} diverged"
        );
    }
}

#[test]
#[ignore = "requires the isolated clocked PulseAudio monitor in scripts/check-audio.sh"]
fn speed_changes_carry_across_the_boundary_without_a_silent_gap() {
    for speed in [0.5_f32, 2.] {
        let capture = Capture::open();
        let mut player = Player::open(wave(RATE, 0, 2 * HALF_SECOND, counter)).unwrap();
        player.speed(speed).unwrap();
        player
            .prepare_next(wave(
                RATE,
                u64::from(2 * HALF_SECOND),
                2 * HALF_SECOND,
                counter,
            ))
            .unwrap();
        player.resume().unwrap();
        wait(|| player.snapshot().ended);
        assert_eq!(player.snapshot().track, 1);
        assert_eq!(player.snapshot().speed, speed);
        let captured = finish(&capture, player);
        let audit = audit(&captured, counter, 1);
        let start = captured.iter().position(|sample| *sample != 0.).unwrap();
        let boundary = start + (2. * f64::from(HALF_SECOND) / f64::from(speed)) as usize;
        let around = &captured[boundary - 4_800..boundary + 4_800];
        let gap = longest_run(around);
        let expected = 4. * f64::from(HALF_SECOND) / f64::from(speed);
        eprintln!("speed {speed} gap frames={gap} heard={}", audit.heard);
        assert!(gap <= 2);
        assert!(
            (audit.heard as f64 - expected).abs() < expected * 0.02,
            "speed {speed} produced {} frames, expected about {expected}",
            audit.heard
        );
    }
}

fn tone(path: &Path, codec: &str, extra: &[&str]) {
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

fn quiet_run(samples: &[f32]) -> usize {
    let (mut longest, mut run) = (0, 0);
    for sample in samples {
        run = if sample.abs() < 0.0005 { run + 1 } else { 0 };
        longest = longest.max(run);
    }
    longest
}

fn lossy_boundary_silence(name: &str, codec: &str, extra: &[&str]) -> usize {
    let root = tempfile::tempdir().unwrap();
    let (first, second) = (root.path().join(name), root.path().join(format!("2{name}")));
    tone(&first, codec, extra);
    tone(&second, codec, extra);
    let capture = Capture::open();
    let mut player = Player::open(File::open(&first).unwrap()).unwrap();
    player.prepare_next(File::open(&second).unwrap()).unwrap();
    player.resume().unwrap();
    wait(|| player.snapshot().ended);
    assert_eq!(player.snapshot().track, 1);
    let captured = finish(&capture, player);
    let start = captured
        .iter()
        .position(|sample| sample.abs() > 0.0005)
        .unwrap();
    let boundary = start + HALF_SECOND as usize;
    quiet_run(&captured[boundary - 4_800..boundary + 4_800])
}

#[test]
#[ignore = "requires the isolated clocked PulseAudio monitor in scripts/check-audio.sh"]
fn lossy_streams_join_without_silence_only_when_their_delay_is_declared() {
    let tagged = lossy_boundary_silence("tagged.mp3", "libmp3lame", &[]);
    let vorbis = lossy_boundary_silence("tagged.ogg", "libvorbis", &[]);
    let untagged = lossy_boundary_silence("untagged.mp3", "libmp3lame", &["-write_xing", "0"]);
    let aac = lossy_boundary_silence("priming.m4a", "aac", &[]);
    eprintln!(
        "mp3 with LAME tag gap frames={tagged}, vorbis gap frames={vorbis}, mp3 without tag gap frames={untagged}, aac gap frames={aac}"
    );
    assert!(tagged <= 16);
    assert!(vorbis <= 16);
    assert!(untagged > 500);
    assert!(aac > 100);
}

#[test]
#[ignore = "requires the isolated clocked PulseAudio monitor in scripts/check-audio.sh"]
fn offering_and_withdrawing_inputs_mid_playback_never_drops_samples() {
    let capture = Capture::open();
    let mut player = Player::open(wave(RATE, 0, 4 * RATE / 2, counter)).unwrap();
    player.resume().unwrap();
    for round in 0..12 {
        let target = Duration::from_millis(60 * (round + 1));
        wait(|| player.snapshot().position >= target);
        player.prepare_next(wave(RATE, 7, 4_800, counter)).unwrap();
        if round % 2 == 0 {
            assert!(player.cancel_next().unwrap());
        }
    }
    assert!(player.cancel_next().unwrap());
    wait(|| player.snapshot().ended);
    let captured = finish(&capture, player);
    let audit = audit(&captured, counter, 4 * RATE as usize / 2);
    assert_eq!(audit.mismatches, 0);
    assert_eq!(audit.longest_silence, 0);
}

#[test]
#[ignore = "requires the isolated clocked PulseAudio monitor in scripts/check-audio.sh"]
fn a_paused_stream_with_a_full_ring_still_answers_next_input_requests() {
    let mut player = Player::open(wave(RATE, 0, 4 * RATE, counter)).unwrap();
    player.resume().unwrap();
    wait(|| player.snapshot().position > Duration::from_millis(200));
    player.pause();
    std::thread::sleep(Duration::from_millis(400));
    let (done, finished) = std::sync::mpsc::channel();
    let worker = std::thread::spawn(move || {
        player.prepare_next(wave(RATE, 9, 4_800, counter)).unwrap();
        assert!(player.cancel_next().unwrap());
        player.seek(Duration::from_millis(500)).unwrap();
        let _ = done.send(());
        player
    });
    finished
        .recv_timeout(Duration::from_secs(3))
        .expect("the paused worker did not answer");
    worker.join().unwrap().close().unwrap();
}
