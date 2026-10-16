use crate::Error;
#[cfg(all(target_os = "linux", not(target_env = "uclibc")))]
use nix::{
    errno::Errno,
    sys::{
        signal::{Signal, killpg},
        wait::{Id, WaitPidFlag, WaitStatus, waitid},
    },
    unistd::Pid,
};
#[cfg(all(target_os = "linux", not(target_env = "uclibc")))]
use std::time::Instant;
use std::{future::Future, path::PathBuf};

#[cfg(all(target_os = "linux", not(target_env = "uclibc")))]
pub(crate) async fn run(
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
pub(crate) async fn run(
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
        target: "hito_plugin_youtube::extractor",
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
