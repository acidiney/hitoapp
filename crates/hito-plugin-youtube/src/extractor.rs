use crate::Error;
use hito_plugin_sdk::{ProviderRecording, SearchPage, SearchRequest};
#[cfg(all(target_os = "linux", not(target_env = "uclibc")))]
use nix::{
    errno::Errno,
    sys::{
        signal::{Signal, killpg},
        wait::{Id, WaitPidFlag, WaitStatus, waitid},
    },
    unistd::Pid,
};
use serde::Deserialize;
use std::{
    future::Future,
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::Instant,
};
use tokio::sync::{Semaphore, oneshot};
use tokio_util::{sync::CancellationToken, task::TaskTracker};
use tracing::{Instrument, instrument::WithSubscriber};

const MAX_RESPONSE_BYTES: usize = 262_144;

pub(crate) struct Extractor {
    executable: PathBuf,
    slots: Arc<Semaphore>,
    admission: Mutex<()>,
    workers: TaskTracker,
    stopped: CancellationToken,
    failed: Arc<AtomicBool>,
}

impl Extractor {
    pub(crate) fn new(executable: PathBuf) -> Self {
        Self {
            executable,
            slots: Arc::new(Semaphore::new(2)),
            admission: Mutex::new(()),
            workers: TaskTracker::new(),
            stopped: CancellationToken::new(),
            failed: Arc::new(AtomicBool::new(false)),
        }
    }

    pub(crate) async fn search(&self, request: SearchRequest) -> Result<SearchPage, Error> {
        let receiver = {
            let _gate = self
                .admission
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            if self.workers.is_closed() || self.failed.load(Ordering::Acquire) {
                return Err(Error::Unavailable);
            }
            let permit = self
                .slots
                .clone()
                .try_acquire_owned()
                .map_err(|_| Error::Busy)?;
            let executable = self.executable.clone();
            let stopped = self.stopped.clone();
            let failed = self.failed.clone();
            let (mut sender, receiver) = oneshot::channel();
            let dispatch = tracing::dispatcher::get_default(Clone::clone);
            self.workers.spawn(
                async move {
                    let _permit = permit;
                    let started = Instant::now();
                    let canceled = async {
                        tokio::select! {
                            biased;
                            _ = stopped.cancelled() => {},
                            _ = sender.closed() => {},
                        }
                    };
                    let result = run(executable, request.query, request.limit, canceled)
                        .await
                        .and_then(|bytes| normalize(&bytes, request.limit));
                    if matches!(result, Err(Error::CleanupFailed)) {
                        failed.store(true, Ordering::Release);
                        stopped.cancel();
                    }
                    tracing::info!(
                        event = "youtube_search_complete",
                        outcome = result
                            .as_ref()
                            .map_or_else(|error| error.outcome(), |_| "complete"),
                        elapsed_ms = started.elapsed().as_millis() as u64
                    );
                    let _ = sender.send(result);
                }
                .instrument(tracing::Span::current())
                .with_subscriber(dispatch),
            );
            receiver
        };
        receiver.await.map_err(|_| Error::Unavailable)?
    }

    pub(crate) async fn shutdown(&self) -> Result<(), Error> {
        static SEQUENCE: AtomicU64 = AtomicU64::new(1);
        let shutdown_id = SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let started = Instant::now();
        {
            let _gate = self
                .admission
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            self.workers.close();
            self.stopped.cancel();
        }
        self.workers.wait().await;
        let failed = self.failed.load(Ordering::Acquire);
        tracing::info!(
            event = "youtube_shutdown_complete",
            shutdown_id,
            outcome = if failed { "cleanup_failed" } else { "complete" },
            elapsed_ms = started.elapsed().as_millis() as u64
        );
        if failed {
            Err(Error::CleanupFailed)
        } else {
            Ok(())
        }
    }
}

#[cfg(all(target_os = "linux", not(target_env = "uclibc")))]
async fn run(
    executable: PathBuf,
    query: String,
    limit: u16,
    canceled: impl Future<Output = ()>,
) -> Result<Vec<u8>, Error> {
    use std::{process::Stdio, time::Duration};
    use tokio::process::Command;

    if !executable.is_absolute() {
        return Err(Error::Unavailable);
    }
    tokio::pin!(canceled);
    tokio::select! {
        biased;
        _ = &mut canceled => return Err(Error::Canceled),
        _ = std::future::ready(()) => {},
    }
    let mut command = Command::new(executable);
    command
        .args([
            "--ignore-config",
            "--no-config-locations",
            "--simulate",
            "--skip-download",
            "--no-cookies",
            "--no-cookies-from-browser",
            "--no-plugin-dirs",
            "--no-remote-components",
            "--no-cache-dir",
            "--dump-single-json",
            "--flat-playlist",
            "--",
        ])
        .arg(format!("ytsearch{limit}:{query}"))
        .env_clear()
        .env("PATH", "/usr/local/bin:/usr/bin:/bin")
        .env("LANG", "C.UTF-8")
        .env("YTDLP_NO_PLUGINS", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .process_group(0);
    let mut child = command.spawn().map_err(|_| Error::Unavailable)?;
    let group = child
        .id()
        .and_then(|id| i32::try_from(id).ok())
        .map(Pid::from_raw)
        .ok_or(Error::Unavailable)?;
    let (Some(stdout), Some(stderr)) = (child.stdout.take(), child.stderr.take()) else {
        stop(&mut child, group).await?;
        return Err(Error::Unavailable);
    };
    let result = {
        let collect = async {
            let (output, _) = tokio::try_join!(
                read_bounded(stdout, 2 * 1024 * 1024),
                read_bounded(stderr, 65_536),
            )?;
            wait_for_exit(group).await?;
            Ok(output)
        };
        tokio::pin!(collect);
        tokio::select! {
            biased;
            _ = &mut canceled => Err(Error::Canceled),
            _ = tokio::time::sleep(Duration::from_secs(8)) => Err(Error::Deadline),
            result = &mut collect => result,
        }
    };
    stop(&mut child, group).await?;
    result
}

#[cfg(not(all(target_os = "linux", not(target_env = "uclibc"))))]
async fn run(
    _executable: PathBuf,
    _query: String,
    _limit: u16,
    _canceled: impl Future<Output = ()>,
) -> Result<Vec<u8>, Error> {
    Err(Error::Unavailable)
}

#[cfg(all(target_os = "linux", not(target_env = "uclibc")))]
async fn read_bounded(
    mut reader: impl tokio::io::AsyncRead + Unpin,
    maximum: usize,
) -> Result<Vec<u8>, Error> {
    use tokio::io::AsyncReadExt;

    let mut output = Vec::new();
    let mut buffer = [0; 8192];
    loop {
        let count = reader
            .read(&mut buffer)
            .await
            .map_err(|_| Error::Unavailable)?;
        if count == 0 {
            return Ok(output);
        }
        if output.len() + count > maximum {
            return Err(Error::InvalidResponse);
        }
        output.extend_from_slice(&buffer[..count]);
    }
}

#[cfg(all(target_os = "linux", not(target_env = "uclibc")))]
fn child_status(group: Pid) -> Result<WaitStatus, Error> {
    loop {
        match waitid(
            Id::Pid(group),
            WaitPidFlag::WEXITED | WaitPidFlag::WNOWAIT | WaitPidFlag::WNOHANG,
        ) {
            Err(Errno::EINTR) => continue,
            result => return result.map_err(|_| Error::Unavailable),
        }
    }
}

#[cfg(all(target_os = "linux", not(target_env = "uclibc")))]
async fn wait_for_exit(group: Pid) -> Result<(), Error> {
    loop {
        match child_status(group)? {
            WaitStatus::StillAlive => {
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
            WaitStatus::Exited(_, 0) => return Ok(()),
            _ => return Err(Error::Unavailable),
        }
    }
}

#[cfg(all(target_os = "linux", not(target_env = "uclibc")))]
async fn stop(child: &mut tokio::process::Child, group: Pid) -> Result<(), Error> {
    let deadline = Instant::now() + std::time::Duration::from_secs(2);
    let group_outcome = if child_status(group).is_ok() {
        let outcome = match killpg(group, Signal::SIGKILL) {
            Ok(()) => "signaled",
            Err(Errno::ESRCH) => "gone",
            Err(_) => "signal_failed",
        };
        let _ = child.start_kill();
        outcome
    } else {
        "ownership_lost"
    };
    let stopped = if matches!(group_outcome, "signaled" | "gone") {
        wait_group_stopped(group, deadline).await.is_ok()
    } else {
        false
    };
    let reaped = matches!(
        tokio::time::timeout_at(tokio::time::Instant::from_std(deadline), child.wait()).await,
        Ok(Ok(_))
    );
    let outcome = if reaped { "reaped" } else { "reap_failed" };
    tracing::debug!(
        event = "youtube_process_stopped",
        outcome,
        group_outcome,
        terminated = stopped
    );
    if reaped && stopped {
        Ok(())
    } else {
        Err(Error::CleanupFailed)
    }
}

#[cfg(all(target_os = "linux", not(target_env = "uclibc")))]
async fn wait_group_stopped(group: Pid, deadline: Instant) -> Result<(), Error> {
    use std::io::Read;

    const MAX_PASS_ENTRIES: usize = 4096;
    const MAX_TOTAL_ENTRIES: usize = 8192;
    const MAX_STAT_BYTES: usize = 4096;

    let mut remaining = MAX_TOTAL_ENTRIES;
    loop {
        let mut running = false;
        for (index, entry) in std::fs::read_dir("/proc")
            .map_err(|_| Error::CleanupFailed)?
            .enumerate()
        {
            if index >= MAX_PASS_ENTRIES || remaining == 0 || Instant::now() >= deadline {
                return Err(Error::CleanupFailed);
            }
            remaining -= 1;
            let entry = entry.map_err(|_| Error::CleanupFailed)?;
            if !entry
                .file_name()
                .as_encoded_bytes()
                .iter()
                .all(u8::is_ascii_digit)
            {
                continue;
            }
            let file = match std::fs::File::open(entry.path().join("stat")) {
                Ok(file) => file,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(_) => return Err(Error::CleanupFailed),
            };
            let mut value = Vec::new();
            match file.take(MAX_STAT_BYTES as u64 + 1).read_to_end(&mut value) {
                Ok(_) => {}
                Err(error)
                    if error.kind() == std::io::ErrorKind::NotFound
                        || error.raw_os_error() == Some(Errno::ESRCH as i32) =>
                {
                    continue;
                }
                Err(_) => return Err(Error::CleanupFailed),
            }
            if value.len() > MAX_STAT_BYTES {
                return Err(Error::CleanupFailed);
            }
            let offset = value
                .windows(2)
                .rposition(|part| part == b") ")
                .ok_or(Error::CleanupFailed)?
                + 2;
            let fields = std::str::from_utf8(&value[offset..]).map_err(|_| Error::CleanupFailed)?;
            let fields: Vec<_> = fields.split_whitespace().collect();
            let state = fields.first().ok_or(Error::CleanupFailed)?;
            let owner: i32 = fields
                .get(2)
                .ok_or(Error::CleanupFailed)?
                .parse()
                .map_err(|_| Error::CleanupFailed)?;
            if owner == group.as_raw() && !matches!(*state, "Z" | "X") {
                running = true;
                break;
            }
        }
        if !running {
            return Ok(());
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
}

#[derive(Deserialize)]
struct SearchResults {
    entries: Vec<SearchEntry>,
}

#[derive(Deserialize)]
struct SearchEntry {
    id: String,
    title: String,
    artist: Option<String>,
    channel: Option<String>,
    uploader: Option<String>,
    duration: Option<f64>,
}

fn normalize(bytes: &[u8], limit: u16) -> Result<SearchPage, Error> {
    let results: SearchResults =
        serde_json::from_slice(bytes).map_err(|_| Error::InvalidResponse)?;
    if results.entries.len() > 50 {
        return Err(Error::InvalidResponse);
    }
    let mut items = Vec::with_capacity(results.entries.len());
    for entry in results.entries {
        if entry.id.len() != 11
            || !entry
                .id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"_-".contains(&byte))
        {
            return Err(Error::InvalidResponse);
        }
        let artist = match entry.artist {
            Some(artist) => artist,
            None => {
                let channel = entry
                    .channel
                    .or(entry.uploader)
                    .ok_or(Error::InvalidResponse)?;
                if channel.trim().is_empty() || channel.chars().any(char::is_control) {
                    return Err(Error::InvalidResponse);
                }
                format!("Channel: {channel}")
            }
        };
        let duration_ms = entry
            .duration
            .map(|duration| {
                let milliseconds = (duration * 1000.0).round();
                if !duration.is_finite() || duration < 0.0 || milliseconds > 9_007_199_254_740_991.0
                {
                    return Err(Error::InvalidResponse);
                }
                Ok(milliseconds as u64)
            })
            .transpose()?;
        items.push(ProviderRecording {
            id: entry.id,
            title: entry.title,
            artist,
            duration_ms,
            release: None,
            disc_number: None,
            track_number: None,
            revision: None,
        });
    }
    let mut page = SearchPage {
        items,
        next_cursor: None,
    };
    page.validate(50).map_err(|_| Error::InvalidResponse)?;
    page.items.truncate(usize::from(limit));
    let length = serde_json::to_vec(&page)
        .map_err(|_| Error::InvalidResponse)?
        .len();
    if length > MAX_RESPONSE_BYTES {
        return Err(Error::InvalidResponse);
    }
    Ok(page)
}
