#![cfg(all(feature = "device", target_os = "linux"))]

use hito_audio::{Player, test_support};
use std::{
    io::{Read, Write},
    time::{Duration, Instant},
};
use test_support::{continuity::counter, output_witness::OutputWitness};

fn event(message: &str) {
    println!("\n{message}");
    std::io::stdout().flush().unwrap();
}

fn command(expected: u8) {
    let mut byte = [0];
    std::io::stdin().read_exact(&mut byte).unwrap();
    assert_eq!(byte[0], expected, "invalid owned controller command");
}

#[test]
#[ignore = "requires the dedicated native_buffer_probe controller and private Pulse launcher"]
fn a_late_backend_write_preserves_every_source_frame() {
    assert_eq!(
        std::env::var("HITO_NATIVE_BUFFER_PROBE").as_deref(),
        Ok("1")
    );
    let witness = OutputWitness::begin(30 * 48_000, 4096).unwrap();
    let input = test_support::wave(48_000, 2, 576_000, |frame, _| {
        counter(u64::from(frame % 288_000))
    });
    let opening = Instant::now();
    let mut player = Player::open(input).unwrap();
    let open_us = opening.elapsed().as_micros();
    assert!(!player.snapshot().playing);
    event(&format!(
        "{{\"event\":\"player_ready\",\"open_us\":{open_us}}}"
    ));
    command(b'S');
    let started = Instant::now();
    player.resume().unwrap();
    let mut first_progress = None;
    let deadline = started + Duration::from_secs(20);
    loop {
        let snapshot = player.snapshot();
        assert!(snapshot.error.is_none(), "public player failed");
        if first_progress.is_none() && !snapshot.position.is_zero() {
            first_progress = Some(started.elapsed().as_micros());
        }
        if snapshot.ended {
            assert_eq!(snapshot.position, Duration::from_secs(12));
            event(&format!(
                "{{\"event\":\"player_complete\",\"source_frames\":576000,\"underruns\":{},\"xruns\":{},\"first_progress_us\":{},\"elapsed_us\":{}}}",
                snapshot.underruns,
                snapshot.xruns,
                first_progress.unwrap(),
                started.elapsed().as_micros()
            ));
            assert_eq!(snapshot.underruns, 0);
            assert_eq!(snapshot.xruns, 0);
            break;
        }
        assert!(Instant::now() < deadline, "public player did not finish");
        std::thread::sleep(Duration::from_millis(1));
    }
    command(b'C');
    event("{\"event\":\"player_close_started\"}");
    let closing = Instant::now();
    player.close().unwrap();
    let report = witness.finish().unwrap();
    test_support::continuity::dump("rendered", &report.samples);
    let mut active = report
        .samples
        .iter()
        .copied()
        .skip_while(|sample| *sample == 0.)
        .collect::<Vec<_>>();
    while active.last() == Some(&0.) {
        active.pop();
    }
    assert_eq!(active.len(), 576_000, "rendered source frame count changed");
    assert!(
        active.iter().enumerate().all(|(frame, sample)| {
            *sample == f32::from(counter((frame % 288_000) as u64)) / 32768.
        }),
        "rendered source order or completeness changed"
    );
    assert!(
        report
            .callbacks
            .iter()
            .all(|callback| callback.owner == 1 && callback.generation == 1)
    );
    event(&format!(
        "{{\"event\":\"player_closed\",\"result\":0,\"close_us\":{},\"callbacks\":{},\"callback_frames\":{}}}",
        closing.elapsed().as_micros(),
        report.callbacks.len(),
        report.callbacks[0].frames
    ));
}

#[derive(Clone, Default)]
struct Logs(std::sync::Arc<std::sync::Mutex<Vec<u8>>>);

impl Write for Logs {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        let mut stored = self.0.lock().unwrap();
        assert!(
            stored.len() + bytes.len() < 64 * 1024,
            "log work budget exceeded"
        );
        stored.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn number(fields: &str, name: &str) -> u64 {
    fields
        .split_once(&format!("\"{name}\":"))
        .expect("required numeric workflow field missing")
        .1
        .split(|character: char| !character.is_ascii_digit())
        .next()
        .unwrap()
        .parse()
        .unwrap()
}

#[test]
#[ignore = "requires the isolated private PulseAudio launcher"]
fn normal_logs_correlate_output_generations_and_report_backend_callback_quantum() {
    let logs = Logs::default();
    let writer = logs.clone();
    let subscriber = tracing_subscriber::fmt()
        .json()
        .with_ansi(false)
        .with_max_level(tracing::Level::INFO)
        .with_writer(move || writer.clone())
        .finish();
    let _subscriber = tracing::subscriber::set_default(subscriber);
    let witness = OutputWitness::begin(4 * 48_000, 1024).unwrap();
    let first = Player::open(test_support::wave(48_000, 2, 2400, |_, _| 1)).unwrap();
    let second = Player::open(test_support::wave(48_000, 2, 2400, |_, _| 2)).unwrap();
    first.close().unwrap();
    second.close().unwrap();
    let report = witness.finish().unwrap();
    let bytes = logs.0.lock().unwrap();
    let text = std::str::from_utf8(&bytes).unwrap();
    let opened: Vec<_> = text
        .lines()
        .filter(|line| line.contains("\"event\":\"audio_output_opened\""))
        .map(|line| {
            line.split_once("\"fields\":{")
                .unwrap()
                .1
                .split_once('}')
                .unwrap()
                .0
        })
        .collect();
    assert_eq!(opened.len(), 2);
    let ids: Vec<_> = opened
        .iter()
        .map(|fields| number(fields, "audio_id"))
        .collect();
    assert!(ids[0] > 0 && ids[1] > 0 && ids[0] != ids[1]);
    for (index, fields) in opened.iter().enumerate() {
        assert_eq!(number(fields, "generation"), 1);
        assert!(fields.contains("\"backend\":\"Alsa\""));
        assert!(number(fields, "requested_quantum_frames") > 0);
        assert!(number(fields, "configured_quantum_frames") > 0);
        assert!(number(fields, "open_us") > 0);
        assert!(fields.contains("\"buffer_constraint\":\"within_range\""));
        let quantum = number(fields, "reported_callback_frames");
        assert!(quantum > 0);
        assert!(
            report
                .callbacks
                .iter()
                .filter(|callback| callback.owner == index as u64 + 1)
                .all(|callback| callback.frames as u64 == quantum)
        );
        assert!(
            text.lines()
                .any(|line| line.contains("\"event\":\"audio_output_closed\"")
                    && number(line, "audio_id") == ids[index]
                    && number(line, "generation") == 1)
        );
        eprintln!(
            "output_open_measurement audio_id={} generation=1 callback_frames={} open_us={}",
            ids[index],
            quantum,
            number(fields, "open_us")
        );
    }
    for forbidden in [
        "device_name",
        "private_path",
        "signed_url",
        "source_title",
        "samples",
    ] {
        assert!(
            !text.contains(forbidden),
            "private media field leaked into workflow logs"
        );
    }
}

#[test]
#[ignore = "requires the isolated private PulseAudio launcher"]
fn a_measured_seek_preserves_the_exact_source_frame_and_replacement_epoch() {
    let witness = OutputWitness::begin(8 * 48_000, 4096).unwrap();
    let input = test_support::wave(48_000, 2, 3 * 48_000, |frame, _| counter(u64::from(frame)));
    let mut player = Player::open(input).unwrap();
    player.resume().unwrap();
    test_support::capture::wait(|| player.snapshot().position >= Duration::from_millis(200));
    let seeking = Instant::now();
    player.seek(Duration::from_secs(1)).unwrap();
    let seek_apply_us = seeking.elapsed().as_micros();
    test_support::capture::wait(|| player.snapshot().position > Duration::from_secs(1));
    let first_progress_us = seeking.elapsed().as_micros();
    test_support::capture::wait(|| player.snapshot().ended || player.snapshot().error.is_some());
    assert!(player.snapshot().error.is_none());
    assert_eq!(player.snapshot().underruns, 0);
    assert_eq!(player.snapshot().xruns, 0);
    player.close().unwrap();
    let report = witness.finish().unwrap();
    let samples: Vec<_> = report
        .callbacks
        .iter()
        .filter(|callback| callback.epoch == 2)
        .flat_map(|callback| {
            report.samples[callback.offset..callback.offset + callback.frames]
                .iter()
                .copied()
        })
        .collect();
    let start = samples.iter().position(|sample| *sample != 0.).unwrap();
    let end = samples.iter().rposition(|sample| *sample != 0.).unwrap();
    assert_eq!(end + 1 - start, 96_000, "seek changed source completeness");
    assert!(
        samples[start..=end]
            .iter()
            .enumerate()
            .all(|(offset, sample)| *sample == f32::from(counter(48_000 + offset as u64)) / 32768.),
        "seek emitted a wrong or stale frame"
    );
    eprintln!(
        "seek_measurement applied_us={seek_apply_us} first_position_us={first_progress_us} source_start_frame=48000 source_frames=96000"
    );
}
