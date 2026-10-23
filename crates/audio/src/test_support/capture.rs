use std::{
    io::Read,
    process::{Child, Command, Stdio},
    sync::{Arc, Mutex},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

pub struct Capture {
    child: Child,
    reading: Option<JoinHandle<()>>,
    samples: Arc<Mutex<Vec<f32>>>,
}

impl Capture {
    #[track_caller]
    pub fn open() -> Self {
        let mut child = Command::new("parec")
            .args([
                "--device=hito-test.monitor",
                "--format=float32le",
                "--rate=48000",
                "--channels=2",
                "--latency-msec=20",
            ])
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap();
        let mut stdout = child.stdout.take().unwrap();
        let samples = Arc::new(Mutex::new(Vec::new()));
        let recording = samples.clone();
        let reading = thread::spawn(move || {
            let mut buffer = [0_u8; 4096];
            let mut pending = Vec::new();
            loop {
                let count = stdout.read(&mut buffer).unwrap();
                if count == 0 {
                    break;
                }
                pending.extend_from_slice(&buffer[..count]);
                let complete = pending.len() / 8 * 8;
                let mut samples = recording.lock().unwrap();
                samples.extend(
                    pending[..complete]
                        .as_chunks::<8>()
                        .0
                        .iter()
                        .map(|frame| f32::from_le_bytes(frame[..4].try_into().unwrap())),
                );
                assert!(
                    samples.len() < 48_000 * 30,
                    "capture exceeded its work budget"
                );
                pending.drain(..complete);
            }
        });
        let capture = Self {
            child,
            reading: Some(reading),
            samples,
        };
        capture.after(0, 0.1);
        capture
    }

    pub fn mark(&self) -> usize {
        self.samples.lock().unwrap().len()
    }

    #[track_caller]
    pub fn after(&self, mark: usize, seconds: f64) {
        wait(|| self.mark() >= mark + (seconds * 48_000.) as usize);
    }

    pub fn recent(&self, seconds: f64) -> Vec<f32> {
        let samples = self.samples.lock().unwrap();
        samples[samples.len().saturating_sub((seconds * 48_000.) as usize)..].to_vec()
    }

    pub fn all(&self) -> Vec<f32> {
        self.samples.lock().unwrap().clone()
    }
}

impl Drop for Capture {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = self.reading.take().unwrap().join();
    }
}

#[track_caller]
pub fn wait(mut condition: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !condition() {
        assert!(Instant::now() < deadline, "audio condition timed out");
        thread::sleep(Duration::from_millis(5));
    }
}

pub fn energy(samples: &[f32]) -> f64 {
    samples
        .iter()
        .map(|sample| f64::from(*sample).powi(2))
        .sum::<f64>()
        / samples.len() as f64
}

pub fn amplitude(samples: &[f32], frequency: f64) -> f64 {
    let mut real = 0.;
    let mut imaginary = 0.;
    for (index, sample) in samples.iter().enumerate() {
        let phase = index as f64 * frequency * std::f64::consts::TAU / 48_000.;
        real += f64::from(*sample) * phase.cos();
        imaginary += f64::from(*sample) * phase.sin();
    }
    real.hypot(imaginary) * 2. / samples.len() as f64
}
