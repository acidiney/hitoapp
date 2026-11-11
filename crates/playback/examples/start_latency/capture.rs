use std::{
    io::Read,
    process::Command,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
#[derive(Default)]
struct Progress {
    frames: u64,
    silent_frames: u64,
}

pub(super) struct Monitor {
    process: super::process::ReaderProcess,
    transitions: Arc<Mutex<Vec<(Instant, bool)>>>,
    armed: Arc<AtomicBool>,
    last_audible: Arc<Mutex<Option<Instant>>>,
    progress: Arc<Mutex<Progress>>,
}

impl Monitor {
    pub(super) fn open() -> Result<Self, super::Failure> {
        let mut command = Command::new("parec");
        command.args([
            "--device=hito-test.monitor",
            "--format=float32le",
            "--rate=48000",
            "--channels=2",
            "--latency-msec=5",
        ]);
        Self::start(command)
    }

    pub(super) fn start(command: Command) -> Result<Self, super::Failure> {
        let transitions = Arc::new(Mutex::new(Vec::new()));
        let armed = Arc::new(AtomicBool::new(false));
        let last_audible = Arc::new(Mutex::new(None));
        let progress = Arc::new(Mutex::new(Progress::default()));
        let read_progress = progress.clone();
        let (recorded, flag, latest) = (transitions.clone(), armed.clone(), last_audible.clone());
        let process = super::process::ReaderProcess::spawn(command, move |mut stdout| {
            read_stream(&mut stdout, &recorded, &flag, &latest, &read_progress)
        })?;
        Ok(Self {
            process,
            transitions,
            armed,
            last_audible,
            progress,
        })
    }

    pub(super) async fn silent_for(&mut self, quiet: Duration) -> Result<(), super::Failure> {
        let deadline = Instant::now() + Duration::from_secs(10);
        self.silent_until(quiet, deadline).await
    }

    pub(super) async fn silent_until(
        &mut self,
        quiet: Duration,
        deadline: Instant,
    ) -> Result<(), super::Failure> {
        let mark = self
            .progress
            .lock()
            .map_err(|_| "monitor_state_failed")?
            .frames;
        let required = (quiet.as_secs_f64() * 48_000.).ceil() as u64;
        loop {
            self.process.check_live()?;
            let silent = {
                let progress = self.progress.lock().map_err(|_| "monitor_state_failed")?;
                let fresh = progress.frames.saturating_sub(mark);
                fresh > 0 && progress.silent_frames.min(fresh) >= required
            };
            if Instant::now() >= deadline {
                return Err("monitor_quiet_deadline".into());
            }
            if silent {
                return Ok(());
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    }

    pub(super) fn arm(&self) {
        self.transitions.lock().unwrap().clear();
        self.armed.store(true, Ordering::SeqCst);
    }

    pub(super) fn finish(self) -> Result<(), super::Failure> {
        self.process.finish()?;
        Ok(())
    }

    pub(super) fn disarm(&self) {
        self.armed.store(false, Ordering::SeqCst);
    }

    pub(super) fn first_audible_after(
        &mut self,
        after: Instant,
    ) -> Result<Option<Instant>, super::Failure> {
        self.process.check_live()?;
        if self
            .last_audible
            .lock()
            .map_err(|_| "monitor_state_failed")?
            .is_none_or(|at| at < after)
        {
            return Ok(None);
        }
        Ok(self
            .transitions
            .lock()
            .unwrap()
            .iter()
            .find(|(at, audible)| *audible && *at >= after)
            .map(|(at, _)| *at))
    }

    pub(super) fn first_silence_after(&self, after: Instant) -> Option<Instant> {
        self.transitions
            .lock()
            .unwrap()
            .iter()
            .find(|(at, audible)| !*audible && *at >= after)
            .map(|(at, _)| *at)
    }
}

fn read_stream(
    stdout: &mut impl Read,
    recorded: &Mutex<Vec<(Instant, bool)>>,
    flag: &AtomicBool,
    latest: &Mutex<Option<Instant>>,
    progress: &Mutex<Progress>,
) -> Result<(), &'static str> {
    let mut buffer = [0_u8; 1928];
    let mut pending = 0;
    let mut state: Option<bool> = None;
    loop {
        let count = pending
            + match stdout.read(&mut buffer[pending..pending + 1920]) {
                Ok(0) => {
                    return if pending == 0 {
                        Ok(())
                    } else {
                        Err("capture_truncated_frame")
                    };
                }
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(_) => return Err("capture_read_failed"),
                Ok(count) => count,
            };
        let complete = count / 8 * 8;
        pending = count - complete;
        if complete == 0 {
            continue;
        }
        let now = Instant::now();
        let audible = buffer[..complete]
            .as_chunks::<4>()
            .0
            .iter()
            .any(|bytes| f32::from_le_bytes(*bytes).abs() > 1e-4);
        buffer.copy_within(complete..count, 0);
        {
            let mut progress = progress.lock().map_err(|_| "monitor_state_failed")?;
            let frames = (complete / 8) as u64;
            progress.frames = progress.frames.saturating_add(frames);
            progress.silent_frames = if audible {
                0
            } else {
                progress.silent_frames.saturating_add(frames)
            };
        }
        if audible {
            *latest.lock().unwrap() = Some(now);
        }
        if flag.load(Ordering::SeqCst) {
            if state != Some(audible) {
                recorded.lock().unwrap().push((now, audible));
            }
            state = Some(audible);
        } else {
            state = None;
        }
    }
}

#[cfg(test)]
#[path = "capture/tests.rs"]
mod tests;
