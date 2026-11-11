use super::*;
use std::{io::Write, os::unix::net::UnixStream};

struct ByteReads<R>(R);
impl<R: Read> Read for ByteReads<R> {
    fn read(&mut self, bytes: &mut [u8]) -> std::io::Result<usize> {
        self.0.read(&mut bytes[..1])
    }
}

#[test]
fn owned_pipe_reassembles_fragmented_stereo_pcm() {
    let (input, mut output) = UnixStream::pair().unwrap();
    let writer = std::thread::spawn(move || {
        for sample in [0.5_f32, 0.5] {
            output.write_all(&sample.to_le_bytes()).unwrap();
        }
    });
    let transitions = Mutex::new(Vec::new());
    let latest = Mutex::new(None);
    read_stream(
        &mut ByteReads(input),
        &transitions,
        &AtomicBool::new(true),
        &latest,
        &Mutex::new(Progress::default()),
    )
    .unwrap();
    writer.join().unwrap();
    assert!(
        latest.lock().unwrap().is_some(),
        "complete audible stereo frame was lost"
    );
    assert_eq!(
        transitions
            .lock()
            .unwrap()
            .iter()
            .map(|(_, audible)| *audible)
            .collect::<Vec<_>>(),
        vec![true]
    );
}
#[tokio::test]
async fn exited_recorder_cannot_be_accepted_as_settled_silence() {
    let mut command = Command::new("python3");
    command.args(["-c", "pass"]);
    let mut monitor = Monitor::start(command).unwrap();
    monitor.process.child.wait().unwrap();
    assert!(
        monitor.silent_for(Duration::ZERO).await.is_err(),
        "exited recorder was accepted as healthy silence"
    );
}

#[test]
fn unreadable_owned_stream_reports_capture_failure() {
    let file = tempfile::NamedTempFile::new().unwrap();
    let mut input = std::fs::OpenOptions::new()
        .write(true)
        .open(file.path())
        .unwrap();
    let result = read_stream(
        &mut input,
        &Mutex::new(Vec::new()),
        &AtomicBool::new(true),
        &Mutex::new(None),
        &Mutex::new(Progress::default()),
    );
    assert!(
        result.is_err(),
        "unreadable stream was accepted as clean EOF"
    );
}

#[test]
fn truncated_stereo_frame_is_a_capture_failure() {
    let (mut input, mut output) = UnixStream::pair().unwrap();
    let writer = std::thread::spawn(move || output.write_all(&[0, 0, 0]).unwrap());
    let result = read_stream(
        &mut input,
        &Mutex::new(Vec::new()),
        &AtomicBool::new(true),
        &Mutex::new(None),
        &Mutex::new(Progress::default()),
    );
    writer.join().unwrap();
    assert!(
        result.is_err(),
        "partial stereo frame was accepted as clean EOF"
    );
}

#[test]
fn child_stdout_pipe_preserves_fragmented_stereo_pcm() {
    let mut command = Command::new("python3");
    command.args([
        "-c",
        "import os; os.write(1,bytes.fromhex('0000003f0000003f'))",
    ]);
    let transitions = Arc::new(Mutex::new(Vec::new()));
    let latest = Arc::new(Mutex::new(None));
    let (recorded, audible) = (transitions.clone(), latest.clone());
    let mut owner = super::super::process::ReaderProcess::spawn(command, move |stdout| {
        read_stream(
            &mut ByteReads(stdout),
            &recorded,
            &AtomicBool::new(true),
            &audible,
            &Mutex::new(Progress::default()),
        )
    })
    .unwrap();
    owner.child.wait().unwrap();
    assert!(
        owner.finish().is_err(),
        "exited pipe producer was accepted as live"
    );
    assert!(
        latest.lock().unwrap().is_some(),
        "fragmented child stdout lost the audible frame"
    );
    assert_eq!(
        transitions
            .lock()
            .unwrap()
            .iter()
            .map(|(_, audible)| *audible)
            .collect::<Vec<_>>(),
        vec![true]
    );
}

#[test]
fn measurement_rejects_an_exited_recorder_even_if_audio_was_buffered() {
    let started = Instant::now();
    let mut command = Command::new("python3");
    command.args([
        "-c",
        "import os; os.write(1,bytes.fromhex('0000003f0000003f'))",
    ]);
    let mut monitor = Monitor::start(command).unwrap();
    monitor.process.child.wait().unwrap();
    let result = monitor.first_audible_after(started);
    assert!(monitor.finish().is_err());
    assert!(
        result.is_err(),
        "measurement accepted data from a terminated observer"
    );
}

#[tokio::test]
async fn live_recorder_without_frames_cannot_prove_quiet_output() {
    let mut command = Command::new("python3");
    command.args(["-c", "import sys; sys.stdin.read()"]);
    let mut monitor = Monitor::start(command).unwrap();
    let observed = tokio::time::timeout(
        Duration::from_millis(30),
        monitor.silent_for(Duration::from_millis(150)),
    )
    .await;
    monitor.finish().unwrap();
    assert!(
        !matches!(observed, Ok(Ok(()))),
        "live recorder with no observed frames proved silence"
    );
}

#[tokio::test]
async fn consecutive_observed_silent_frames_prove_the_same_quiet_window() {
    let mut command = Command::new("python3");
    command.args(["-c", "import os\nwhile True: os.write(1,bytes(1920))"]);
    let mut monitor = Monitor::start(command).unwrap();
    let observed = tokio::time::timeout(
        Duration::from_secs(2),
        monitor.silent_for(Duration::from_millis(150)),
    )
    .await;
    let cleanup = monitor.finish();
    assert!(cleanup.is_ok(), "silent recorder cleanup failed");
    assert!(
        matches!(observed, Ok(Ok(()))),
        "observed silent PCM did not satisfy the quiet window"
    );
}

#[tokio::test]
async fn consecutive_audible_frames_cannot_prove_a_quiet_window() {
    let mut command = Command::new("python3");
    command.args([
        "-c",
        "import os\nwhile True: os.write(1,bytes.fromhex('0000003f0000003f')*240)",
    ]);
    let mut monitor = Monitor::start(command).unwrap();
    let observed = tokio::time::timeout(
        Duration::from_millis(30),
        monitor.silent_for(Duration::from_millis(150)),
    )
    .await;
    let cleanup = monitor.finish();
    assert!(cleanup.is_ok(), "audible recorder cleanup failed");
    assert!(
        !matches!(observed, Ok(Ok(()))),
        "audible PCM satisfied the quiet window"
    );
}
async fn frames_arrived(progress: &Arc<Mutex<Progress>>, frames: u64) -> bool {
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            if progress.lock().unwrap().frames >= frames {
                return;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .is_ok()
}

#[tokio::test]
async fn a_new_quiet_window_requires_all_silent_frames_after_its_mark() {
    use std::{future::Future, task::Poll};
    let mut command = Command::new("python3");
    command.args(["-c", "import sys; out=sys.stdout.buffer; out.write(bytes(7200*8)); out.flush(); sys.stdin.readline(); out.write(bytes(8)); out.flush(); sys.stdin.readline(); out.write(bytes(7199*8)); out.flush(); sys.stdin.read()"]);
    let mut monitor = Monitor::start(command).unwrap();
    let mut input = monitor.process.child.stdin.take().unwrap();
    let progress = monitor.progress.clone();
    let observed: Result<(bool, bool), super::super::Failure> = async {
        if !frames_arrived(&progress, 7200).await {
            return Err("prefill_not_observed".into());
        }
        let check = monitor.silent_for(Duration::from_millis(150));
        tokio::pin!(check);
        let armed =
            std::future::poll_fn(|cx| Poll::Ready(check.as_mut().poll(cx).is_pending())).await;
        if !armed {
            return Err("quiet_check_not_armed".into());
        }
        input
            .write_all(b"few\n")
            .map_err(|_| "control_write_failed")?;
        if !frames_arrived(&progress, 7201).await {
            return Err("fresh_frame_not_observed".into());
        }
        let pending = tokio::time::timeout(Duration::from_millis(30), check.as_mut())
            .await
            .is_err();
        let full_interval_accepted = if pending {
            input
                .write_all(b"rest\n")
                .map_err(|_| "control_write_failed")?;
            matches!(
                tokio::time::timeout(Duration::from_secs(3), check.as_mut()).await,
                Ok(Ok(()))
            )
        } else {
            false
        };
        Ok((pending, full_interval_accepted))
    }
    .await;
    let cleanup = monitor.finish();
    drop(input);
    assert!(cleanup.is_ok(), "fresh-window recorder cleanup failed");
    let (pending, full_interval_accepted) =
        observed.expect("owned fresh-window control did not reach observations");
    assert!(
        pending,
        "historical silence plus one fresh frame satisfied a new150ms window"
    );
    assert!(
        full_interval_accepted,
        "a complete fresh continuous interval was rejected"
    );
}

#[tokio::test]
async fn quiet_frames_observed_by_a_late_poll_cannot_cross_original_deadline() {
    use std::{future::Future, task::Poll};
    let mut command = Command::new("python3");
    command.args(["-c", "import sys; sys.stdin.readline(); sys.stdout.buffer.write(bytes(7200*8)); sys.stdout.buffer.flush(); sys.stdin.read()"]);
    let mut monitor = Monitor::start(command).unwrap();
    let mut input = monitor.process.child.stdin.take().unwrap();
    let progress = monitor.progress.clone();
    let observed: Result<bool, super::super::Failure> = async {
        let deadline = Instant::now() + Duration::from_secs(1);
        let check = monitor.silent_until(Duration::from_millis(150), deadline);
        tokio::pin!(check);
        let armed =
            std::future::poll_fn(|cx| Poll::Ready(check.as_mut().poll(cx).is_pending())).await;
        if !armed {
            return Err("quiet_check_not_armed".into());
        }
        input
            .write_all(b"full\n")
            .map_err(|_| "control_write_failed")?;
        if !frames_arrived(&progress, 7200).await {
            return Err("full_interval_not_observed".into());
        }
        {
            let gate = (Mutex::new(()), std::sync::Condvar::new());
            let mut guard = gate.0.lock().unwrap();
            while let Some(remaining) = deadline.checked_duration_since(Instant::now()) {
                guard = gate.1.wait_timeout(guard, remaining).unwrap().0;
            }
        }
        let result = tokio::time::timeout(Duration::from_secs(3), check.as_mut())
            .await
            .map_err(|_| "late_check_not_completed")?;
        Ok(result.is_err())
    }
    .await;
    let cleanup = monitor.finish();
    drop(input);
    assert!(cleanup.is_ok(), "late-poll recorder cleanup failed");
    assert!(
        observed.expect("owned late-poll control did not reach observation"),
        "quiet check accepted a full interval after its original deadline"
    );
}
