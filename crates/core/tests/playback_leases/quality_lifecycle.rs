use super::work::fixture::Case;
use crate::{
    AudioRequest, OpenedAudio, QualityAudio, QualityRequest, StreamQuality,
    test_support::{hang_guard::hang_guard, media_tool},
};
use std::{
    path::{Path, PathBuf},
    sync::{Arc, atomic::Ordering},
    time::Duration,
};

struct ProbeGate {
    armed: PathBuf,
    entered: PathBuf,
    gate: PathBuf,
}

impl ProbeGate {
    fn install(directory: &Path) -> (Self, PathBuf) {
        let gate = Self {
            armed: directory.join("probe-armed"),
            entered: directory.join("probe-entered"),
            gate: directory.join("probe-gate"),
        };
        for fifo in [&gate.entered, &gate.gate] {
            assert!(
                std::process::Command::new("mkfifo")
                    .arg(fifo)
                    .status()
                    .unwrap()
                    .success()
            );
        }
        let script = media_tool::script(
            directory,
            "ffprobe",
            &format!(
                "if [ -e {armed} ]; then rm {armed}; echo $$ > {entered}; read held < {gate}; fi\nexec /usr/bin/ffprobe \"$@\"",
                armed = media_tool::quote(&gate.armed),
                entered = media_tool::quote(&gate.entered),
                gate = media_tool::quote(&gate.gate),
            ),
        );
        (gate, script)
    }

    fn arm(&self) {
        std::fs::write(&self.armed, b"").unwrap();
    }

    async fn entered(&self) -> u32 {
        let entered = self.entered.clone();
        tokio::task::spawn_blocking(move || std::fs::read_to_string(entered).unwrap())
            .await
            .unwrap()
            .trim()
            .parse()
            .unwrap()
    }
}

fn alive(pid: u32) -> bool {
    std::process::Command::new("kill")
        .args(["-0", &pid.to_string()])
        .stderr(std::process::Stdio::null())
        .status()
        .unwrap()
        .success()
}

fn end(pid: u32) {
    if alive(pid) {
        let _ = std::process::Command::new("kill")
            .args(["-9", &pid.to_string()])
            .status();
    }
}

fn quality(head: bool) -> QualityRequest {
    QualityRequest {
        quality: StreamQuality::DataSaver,
        start: Duration::ZERO,
        original: AudioRequest { head, range: None },
    }
}

async fn gated_case() -> (Case, ProbeGate) {
    let mut installed = None;
    let f = Case::configured(|directory, config| {
        let (gate, script) = ProbeGate::install(directory);
        config.media_tools.ffprobe = script;
        installed = Some(gate);
    })
    .await;
    (f, installed.unwrap())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn releasing_a_lease_joins_its_pending_conversion_probe() {
    let (f, gate) = hang_guard("the gated lease fixture must start", gated_case()).await;
    let lease = f
        .services
        .playback
        .begin_lease(&f.actor, f.request())
        .await
        .unwrap();
    gate.arm();
    let opening = {
        let playback = f.services.playback.clone();
        let actor = f.actor.clone();
        tokio::spawn(async move {
            playback
                .open_lease_quality_audio(&actor, lease.id, quality(true))
                .await
        })
    };
    let pid = hang_guard("the probe must reach its gate", gate.entered()).await;
    let released = tokio::time::timeout(
        Duration::from_secs(5),
        f.services.playback.release_lease(&f.actor, lease.id),
    )
    .await;
    let probe_alive_after_release = alive(pid);
    end(pid);
    let opened = tokio::time::timeout(Duration::from_secs(5), opening).await;
    f.close().await;
    assert!(matches!(released, Ok(Ok(()))));
    assert!(
        !probe_alive_after_release,
        "release must retire and join the pending probe"
    );
    assert!(matches!(opened, Ok(Ok(Err(_)))));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn releasing_a_lease_waits_for_a_blocked_conversion_read() {
    let f = hang_guard("the lease fixture must start", Case::new()).await;
    let pause = Arc::new(super::testing::ReadPause::default());
    f.services.playback.leases.state.lock().unwrap().read_pause = Some(pause.clone());
    let lease = f
        .services
        .playback
        .begin_lease(&f.actor, f.request())
        .await
        .unwrap();
    let entry = f
        .services
        .playback
        .leases
        .entry(&f.actor, lease.id)
        .unwrap();
    let opening = {
        let playback = f.services.playback.clone();
        let actor = f.actor.clone();
        tokio::spawn(async move {
            playback
                .open_lease_quality_audio(&actor, lease.id, quality(false))
                .await
        })
    };
    hang_guard("the conversion read must reach its gate", pause.entered()).await;
    let mut releasing = {
        let playback = f.services.playback.clone();
        let actor = f.actor.clone();
        tokio::spawn(async move { playback.release_lease(&actor, lease.id).await })
    };
    let early = tokio::time::timeout(Duration::from_millis(300), &mut releasing).await;
    let finished_while_reading = early.is_ok();
    pause.release();
    let released = match early {
        Ok(result) => Ok(result),
        Err(_) => tokio::time::timeout(Duration::from_secs(5), releasing).await,
    };
    let reads_after_release = entry.positioned_reads.load(Ordering::SeqCst);
    let reads_started = entry.positioned_started.load(Ordering::SeqCst);
    let opened = tokio::time::timeout(Duration::from_secs(5), opening).await;
    drop(entry);
    f.close().await;
    assert!(
        !finished_while_reading,
        "release must wait for the blocked conversion read"
    );
    assert!(matches!(released, Ok(Ok(Ok(())))));
    assert_eq!(reads_after_release, 0);
    assert_eq!(
        reads_started, 1,
        "no new conversion read may start after cancellation"
    );
    assert!(opened.is_ok());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_converted_chunk_is_withheld_after_the_original_changes_in_place() {
    let f = hang_guard("the lease fixture must start", Case::new()).await;
    let lease = f
        .services
        .playback
        .begin_lease(&f.actor, f.request())
        .await
        .unwrap();
    let opened = f
        .services
        .playback
        .open_lease_quality_audio(&f.actor, lease.id, quality(false))
        .await
        .unwrap();
    let QualityAudio::Converted(mut audio) = opened else {
        panic!("a lossless original converts")
    };
    let mut body = audio.body.take().unwrap();
    {
        use std::os::unix::fs::FileExt;
        let file = std::fs::OpenOptions::new()
            .write(true)
            .open(f.path())
            .unwrap();
        file.write_at(&[0x55; 64], 200).unwrap();
    }
    let first = body.chunk().await;
    let reopened = f
        .services
        .playback
        .open_lease_audio(&f.actor, lease.id, AudioRequest::default())
        .await;
    drop(body);
    f.close().await;
    assert!(
        first.is_err(),
        "a changed original must not publish converted bytes"
    );
    assert!(reopened.is_err());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_existing_body_keeps_reading_while_a_second_quality_probe_waits() {
    let (f, gate) = hang_guard("the gated lease fixture must start", gated_case()).await;
    let lease = f
        .services
        .playback
        .begin_lease(&f.actor, f.request())
        .await
        .unwrap();
    let OpenedAudio::Content(mut audio) = f
        .services
        .playback
        .open_lease_audio(&f.actor, lease.id, AudioRequest::default())
        .await
        .unwrap()
    else {
        panic!("original content")
    };
    let mut body = audio.body.take().unwrap();
    gate.arm();
    let probing = {
        let playback = f.services.playback.clone();
        let actor = f.actor.clone();
        tokio::spawn(async move {
            playback
                .open_lease_quality_audio(&actor, lease.id, quality(true))
                .await
        })
    };
    let pid = hang_guard("the second probe must reach its gate", gate.entered()).await;
    let chunk = tokio::time::timeout(Duration::from_secs(3), body.chunk()).await;
    let released = tokio::time::timeout(
        Duration::from_secs(5),
        f.services.playback.release_lease(&f.actor, lease.id),
    )
    .await;
    end(pid);
    let _ = tokio::time::timeout(Duration::from_secs(5), probing).await;
    drop(body);
    f.close().await;
    assert!(
        matches!(chunk, Ok(Ok(Some(_)))),
        "the first body must not wait for a second probe"
    );
    assert!(matches!(released, Ok(Ok(()))));
}
