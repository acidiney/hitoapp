#![cfg(all(feature = "device", target_os = "linux"))]

use hito_audio::{Player, test_support};
use std::{
    io::Write,
    os::unix::fs::MetadataExt,
    process::Command,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use test_support::{
    capture::{Capture, wait},
    continuity::{counter, dump, heard, level},
    output_witness::OutputWitness,
};

const RATE: u32 = 48_000;
const FRAMES: u32 = 4 * RATE;

#[derive(Clone, Default)]
struct Logs(Arc<Mutex<Vec<u8>>>);

impl Write for Logs {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl Logs {
    fn recovery_targets(&self) -> Vec<u64> {
        let bytes = self.0.lock().unwrap();
        let text = std::str::from_utf8(&bytes).unwrap();
        text.lines()
            .filter(|line| line.contains("\"event\":\"audio_output_recovered\""))
            .map(|line| {
                let value = line
                    .split_once("\"target_position_ns\":")
                    .expect("recovery must report its exact safe source position")
                    .1;
                value
                    .split(|character: char| !character.is_ascii_digit())
                    .next()
                    .unwrap()
                    .parse()
                    .unwrap()
            })
            .collect()
    }
}

struct PausedBackend {
    pid: u32,
    parent: u32,
    resumed: bool,
}

impl PausedBackend {
    fn stop() -> Self {
        let pid: u32 = std::env::var("HITO_PRIVATE_PULSE_PID")
            .expect("the isolated launcher must expose its owned PulseAudio PID")
            .parse()
            .unwrap();
        assert!(pid > 1 && pid != std::process::id());
        let process = format!("/proc/{pid}");
        let parent = |process: &str| -> u32 {
            std::fs::read_to_string(format!("{process}/status"))
                .unwrap()
                .lines()
                .find_map(|line| line.strip_prefix("PPid:"))
                .unwrap()
                .trim()
                .parse()
                .unwrap()
        };
        let backend_parent = parent(&process);
        let test_parent = parent("/proc/self");
        for pid in [backend_parent, test_parent] {
            assert_eq!(
                std::fs::read_to_string(format!("/proc/{pid}/comm"))
                    .unwrap()
                    .trim(),
                "runuser"
            );
        }
        assert_eq!(
            parent(&format!("/proc/{backend_parent}")),
            parent(&format!("/proc/{test_parent}")),
            "the backend and test must share the same isolated launcher"
        );
        assert_eq!(
            std::fs::metadata(&process).unwrap().uid(),
            std::fs::metadata("/proc/self").unwrap().uid(),
            "the backend must belong to the fixture account"
        );
        assert_eq!(
            std::fs::read_to_string(format!("{process}/comm"))
                .unwrap()
                .trim(),
            "pulseaudio"
        );
        let socket = std::env::var("PULSE_SERVER").unwrap();
        let socket = socket.strip_prefix("unix:").unwrap();
        let command = std::fs::read(format!("{process}/cmdline")).unwrap();
        let expected =
            format!("--load=module-native-protocol-unix socket={socket} auth-anonymous=1");
        assert!(
            command
                .split(|byte| *byte == 0)
                .any(|argument| argument == expected.as_bytes()),
            "only the private fixture backend may be interrupted"
        );
        assert!(
            Command::new("kill")
                .args(["-STOP", &pid.to_string()])
                .status()
                .unwrap()
                .success()
        );
        Self {
            pid,
            parent: backend_parent,
            resumed: false,
        }
    }
    fn resume(mut self) {
        assert!(
            Command::new("kill")
                .args(["-CONT", &self.pid.to_string(), &self.parent.to_string()])
                .status()
                .unwrap()
                .success()
        );
        self.resumed = true;
    }
}

impl Drop for PausedBackend {
    fn drop(&mut self) {
        if !self.resumed {
            let _ = Command::new("kill")
                .args(["-CONT", &self.pid.to_string(), &self.parent.to_string()])
                .status();
        }
    }
}

struct Generation {
    number: u64,
    samples: Vec<f32>,
    backend_delay_ns: u64,
    callback_frames: u64,
}

impl Generation {
    fn replay_bound_frames(&self) -> u64 {
        (self.backend_delay_ns * u64::from(RATE)).div_ceil(1_000_000_000) + 2 * self.callback_frames
    }
}

#[test]
#[ignore = "requires the isolated private PulseAudio launcher and its owned backend PID"]
fn an_interrupted_backend_recovers_from_the_observed_source_position_and_joins() {
    let logs = Logs::default();
    let writer = logs.clone();
    let subscriber = tracing_subscriber::fmt()
        .json()
        .with_ansi(false)
        .with_max_level(tracing::Level::DEBUG)
        .with_writer(move || writer.clone())
        .finish();
    let _subscriber = tracing::subscriber::set_default(subscriber);
    let witness = OutputWitness::begin(8 * RATE as usize, 4096).unwrap();
    let capture = Capture::open();
    let mut player = Player::open(test_support::wave(RATE, 2, FRAMES, |frame, _| {
        counter(u64::from(frame))
    }))
    .unwrap();
    player.resume().unwrap();
    wait(|| player.snapshot().position >= Duration::from_millis(500));
    let before = player.snapshot();
    let fault_started = Instant::now();
    let backend = PausedBackend::stop();
    std::thread::sleep(Duration::from_millis(200));
    backend.resume();
    let fault_elapsed = fault_started.elapsed();
    wait(|| player.snapshot().xruns > before.xruns || player.snapshot().error.is_some());
    assert!(player.snapshot().error.is_none());
    wait(|| player.snapshot().ended || player.snapshot().error.is_some());
    let final_state = player.snapshot();
    assert!(final_state.error.is_none());
    assert_eq!(final_state.position, Duration::from_secs(4));
    let closing = Instant::now();
    player.close().unwrap();
    assert!(closing.elapsed() < Duration::from_secs(1));
    let report = witness.finish().unwrap();
    capture.after(capture.mark(), 0.3);
    let monitored = capture.all();
    dump("controlled-recovery-callback", &report.samples);
    dump("controlled-recovery-monitor", &monitored);
    if let Some(directory) = std::env::var_os("HITO_CAPTURE_DUMP") {
        std::fs::write(
            std::path::Path::new(&directory).join("controlled-recovery.log"),
            &*logs.0.lock().unwrap(),
        )
        .unwrap();
    }
    let targets = logs.recovery_targets();
    assert!(
        !targets.is_empty(),
        "the owned backend fault must cause recovery"
    );
    assert_eq!(targets.len() as u64, final_state.xruns - before.xruns);
    let first_target = Duration::from_nanos(targets[0]);
    let mut generations: Vec<Generation> = Vec::new();
    for callback in &report.callbacks {
        if generations
            .last()
            .is_none_or(|entry| entry.number != callback.generation)
        {
            generations.push(Generation {
                number: callback.generation,
                samples: Vec::new(),
                backend_delay_ns: 0,
                callback_frames: 0,
            });
        }
        let generation = generations.last_mut().unwrap();
        generation.callback_frames = generation.callback_frames.max(callback.frames as u64);
        generation.backend_delay_ns = generation.backend_delay_ns.max(
            callback
                .playback_ns
                .saturating_sub(callback.callback_ns)
                .max(0) as u64,
        );
        generation
            .samples
            .extend_from_slice(&report.samples[callback.offset..callback.offset + callback.frames]);
    }
    assert_eq!(generations.len(), targets.len() + 1);
    let buffering_bound =
        Duration::from_secs_f64(generations[0].replay_bound_frames() as f64 / f64::from(RATE));
    assert!(first_target >= before.position);
    assert!(first_target <= before.position + fault_elapsed + buffering_bound);
    assert_eq!(final_state.underruns, 0);
    let mut expected_monitor = Vec::new();
    let mut previous_end = 0;
    let mut replayed_frames = 0;
    for (index, generation) in generations.iter().enumerate() {
        let start = index.checked_sub(1).map_or(0, |previous| {
            ((u128::from(targets[previous]) * u128::from(RATE) + 500_000_000) / 1_000_000_000)
                as u64
        });
        let audio = heard(&generation.samples);
        assert!(!audio.is_empty());
        for (offset, sample) in audio.iter().enumerate() {
            assert!(
                (level(*sample) - i32::from(counter(start + offset as u64))).abs() <= 1,
                "source frames changed inside output generation {index} at {offset}"
            );
        }
        if index > 0 {
            assert!(
                start <= previous_end,
                "recovery skipped submitted source frames"
            );
            let overlap = previous_end - start;
            assert!(
                overlap <= generations[index - 1].replay_bound_frames(),
                "recovery replay exceeded reported backend buffering and callback observation granularity"
            );
            replayed_frames += overlap;
        }
        previous_end = start + audio.len() as u64;
        assert!(previous_end <= u64::from(FRAMES));
        if index + 1 == generations.len() {
            assert_eq!(start + audio.len() as u64, u64::from(FRAMES));
        }
        expected_monitor.extend(audio);
    }
    let audible = heard(&monitored);
    assert_eq!(audible.len(), expected_monitor.len());
    for (index, (audible, submitted)) in audible.iter().zip(&expected_monitor).enumerate() {
        assert!(
            (level(*audible) - level(*submitted)).abs() <= 1,
            "backend capture diverged from submitted output at {index}"
        );
    }
    eprintln!(
        "controlled_recovery recoveries={} first_target_ns={} observed_before_ns={} submitted_frames={} monitor_frames={} already_heard_replayed_frames={} submitted_not_heard_frames=0 underruns={}",
        targets.len(),
        targets[0],
        before.position.as_nanos(),
        expected_monitor.len(),
        audible.len(),
        replayed_frames,
        final_state.underruns
    );
}

#[test]
#[ignore = "requires the isolated private PulseAudio launcher and its owned backend PID"]
fn concurrent_players_identify_recoveries_when_debug_spans_are_disabled() {
    let logs = Logs::default();
    let writer = logs.clone();
    let subscriber = tracing_subscriber::fmt()
        .json()
        .with_ansi(false)
        .with_max_level(tracing::Level::INFO)
        .with_writer(move || writer.clone())
        .finish();
    let _subscriber = tracing::subscriber::set_default(subscriber);
    let mut first = Player::open(test_support::wave(RATE, 2, 3 * RATE, |_, _| 1000)).unwrap();
    let mut second = Player::open(test_support::wave(RATE, 2, 3 * RATE, |_, _| 2000)).unwrap();
    first.resume().unwrap();
    second.resume().unwrap();
    wait(|| {
        first.snapshot().position >= Duration::from_millis(200)
            && second.snapshot().position >= Duration::from_millis(200)
    });
    let positions = [first.snapshot().position, second.snapshot().position];
    let backend = PausedBackend::stop();
    std::thread::sleep(Duration::from_millis(200));
    backend.resume();
    wait(|| {
        first.snapshot().error.is_some()
            || second.snapshot().error.is_some()
            || (first.snapshot().xruns > 0
                && second.snapshot().xruns > 0
                && first.snapshot().position > positions[0] + Duration::from_millis(100)
                && second.snapshot().position > positions[1] + Duration::from_millis(100))
    });
    assert!(first.snapshot().error.is_none());
    assert!(second.snapshot().error.is_none());
    first.close().unwrap();
    second.close().unwrap();
    let bytes = logs.0.lock().unwrap();
    let text = std::str::from_utf8(&bytes).unwrap();
    let mut ids: Vec<u64> = text
        .lines()
        .filter(|line| line.contains("\"event\":\"audio_output_recovered\""))
        .map(|line| {
            let fields = line.split_once("\"fields\":{").unwrap().1;
            let fields = fields.split_once('}').unwrap().0;
            fields
                .split_once("\"audio_id\":")
                .expect("normal-filter recovery events must identify their owning player")
                .1
                .split(|character: char| !character.is_ascii_digit())
                .next()
                .unwrap()
                .parse()
                .unwrap()
        })
        .collect();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), 2);
    assert!(ids.iter().all(|id| *id > 0));
    eprintln!("normal_filter_recovery distinct_audio_ids={}", ids.len());
}
