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
use std::{future::Future, path::PathBuf, sync::Arc, time::Instant};
use tokio::sync::{Semaphore, oneshot};
use tracing::{Instrument, instrument::WithSubscriber};

const MAX_RESPONSE_BYTES: usize = 262_144;

pub(crate) struct Extractor {
    executable: PathBuf,
    slots: Arc<Semaphore>,
}

impl Extractor {
    pub(crate) fn new(executable: PathBuf) -> Self {
        Self {
            executable,
            slots: Arc::new(Semaphore::new(2)),
        }
    }

    pub(crate) async fn search(&self, request: SearchRequest) -> Result<SearchPage, Error> {
        let permit = self
            .slots
            .clone()
            .try_acquire_owned()
            .map_err(|_| Error::Busy)?;
        let executable = self.executable.clone();
        let (mut sender, receiver) = oneshot::channel();
        let dispatch = tracing::dispatcher::get_default(Clone::clone);
        tokio::spawn(
            async move {
                let _permit = permit;
                let started = Instant::now();
                let result = run(executable, request.query, request.limit, sender.closed())
                    .await
                    .and_then(|bytes| normalize(&bytes, request.limit));
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
        receiver.await.map_err(|_| Error::Unavailable)?
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
    let reaped = child.wait().await.is_ok();
    let outcome = if reaped { "reaped" } else { "reap_failed" };
    tracing::debug!(event = "youtube_process_stopped", outcome, group_outcome);
    if reaped && matches!(group_outcome, "signaled" | "gone") {
        Ok(())
    } else {
        Err(Error::Unavailable)
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
