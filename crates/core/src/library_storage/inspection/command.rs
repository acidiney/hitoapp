use super::ownership::Scope;
use crate::{Error, Result};
use std::{ffi::OsString, path::Path, process::Stdio, sync::Arc, time::Duration};
use tokio::{
    io::{AsyncRead, AsyncReadExt},
    process::{Child, Command},
    sync::{Semaphore, oneshot},
    time::Instant,
};
use tracing::Instrument;

#[derive(Clone)]
pub(super) struct Commands {
    slots: Arc<Semaphore>,
    retirement: Option<Scope>,
}

type Outcome<T> = std::result::Result<T, &'static str>;
type Completion = std::result::Result<Vec<u8>, Failure>;

pub(super) enum Failure {
    OutputLimit,
    Other(Error),
}

impl Failure {
    pub(super) fn into_error(self) -> Error {
        match self {
            Self::OutputLimit => Error::Internal("Media inspection could not finish".into()),
            Self::Other(error) => error,
        }
    }
}

impl From<Error> for Failure {
    fn from(error: Error) -> Self {
        Self::Other(error)
    }
}

impl Commands {
    pub(super) fn new() -> Self {
        Self {
            slots: Arc::new(Semaphore::new(2)),
            retirement: None,
        }
    }

    pub(super) fn scoped(&self, retirement: Scope) -> Self {
        Self {
            slots: self.slots.clone(),
            retirement: Some(retirement),
        }
    }

    pub(super) async fn settle(&self) -> Result<()> {
        let _idle = self
            .slots
            .acquire_many(2)
            .await
            .map_err(|_| Error::Internal("Media inspection stopped".into()))?;
        Ok(())
    }

    pub(super) async fn run(
        &self,
        program: &Path,
        arguments: Vec<OsString>,
        limit: usize,
        timeout: Duration,
        media_guard: Option<Arc<tempfile::TempDir>>,
    ) -> Result<Vec<u8>> {
        self.run_until(
            program,
            arguments,
            limit,
            Instant::now() + timeout,
            media_guard,
        )
        .await
        .map_err(Failure::into_error)
    }

    pub(super) async fn run_until(
        &self,
        program: &Path,
        arguments: Vec<OsString>,
        limit: usize,
        deadline: Instant,
        media_guard: Option<Arc<tempfile::TempDir>>,
    ) -> Completion {
        let started = Instant::now();
        let operation = async {
            let program = program.to_owned();
            let permit = self
                .slots
                .clone()
                .acquire_owned()
                .await
                .map_err(|_| Error::Internal("Media inspection stopped".into()))?;
            let retirement = match &self.retirement {
                Some(scope) => Some(scope.acquire().await?),
                None => None,
            };
            if Instant::now() >= deadline {
                return Err(timed_out(started).into());
            }
            let (mut sender, receiver) = oneshot::channel();
            let task = async move {
                let _permit = permit;
                let _retirement = retirement;
                if sender.is_closed() {
                    drop(media_guard);
                    return;
                }
                if Instant::now() >= deadline {
                    drop(media_guard);
                    let _ = sender.send(Err(timed_out(started).into()));
                    return;
                }
                let child = Command::new(program)
                    .args(arguments)
                    .env_remove("FFREPORT")
                    .env("LC_ALL", "C")
                    .stdin(Stdio::null())
                    .stderr(Stdio::piped())
                    .stdout(Stdio::piped())
                    .kill_on_drop(true)
                    .spawn();
                let result = match child {
                    Ok(mut child) => {
                        match supervise(&mut child, limit, deadline, &mut sender).await {
                            Ok(output) => Ok(output),
                            Err(reason) => {
                                tracing::debug!(
                                    event = "media_command_failed",
                                    reason,
                                    "Media command stopped"
                                );
                                Err(match reason {
                                    "output_limit" => Failure::OutputLimit,
                                    _ => Failure::Other(Error::Internal(
                                        "Media inspection could not finish".into(),
                                    )),
                                })
                            }
                        }
                    }
                    Err(spawn_error) => {
                        tracing::warn!(
                            event = "media_tool_unavailable",
                            io_kind = ?spawn_error.kind(),
                            "Media inspection tool unavailable"
                        );
                        Err(Failure::Other(match spawn_error.kind() {
                            std::io::ErrorKind::NotFound | std::io::ErrorKind::PermissionDenied => {
                                Error::MediaToolsMissing
                            }
                            _ => Error::Internal("Media inspection tool is unavailable".into()),
                        }))
                    }
                };
                drop(media_guard);
                let _ = sender.send(result);
            };
            tokio::spawn(task.instrument(tracing::Span::current()));
            let result = receiver
                .await
                .map_err(|_| Error::Internal("Media inspection stopped".into()))?;
            if Instant::now() >= deadline {
                return Err(timed_out(started).into());
            }
            result
        };
        tokio::time::timeout_at(deadline, operation)
            .await
            .map_err(|_| Failure::Other(timed_out(started)))?
    }
}

async fn supervise(
    child: &mut Child,
    limit: usize,
    deadline: Instant,
    sender: &mut oneshot::Sender<Completion>,
) -> Outcome<Vec<u8>> {
    let stdout = child.stdout.take();
    let mut stderr = child.stderr.take();
    let result = {
        let operation = async {
            let stdout = stdout.ok_or("pipe")?;
            let stderr = stderr.as_mut().ok_or("pipe")?;
            let (output, ()) =
                tokio::try_join!(read_output(stdout, limit), reject_diagnostics(stderr))?;
            let status = child.wait().await.map_err(|_| "wait")?;
            if !status.success() {
                return Err("exit");
            }
            Ok(output)
        };
        tokio::select! {
            result = tokio::time::timeout_at(deadline, operation) => result.unwrap_or(Err("timeout")),
            _ = sender.closed() => Err("cancelled"),
        }
    };
    let Err(reason) = result else {
        return result;
    };
    let observed_status = child.try_wait();
    let _ = child.kill().await;
    let reaped = child.wait().await;
    if reason != "output_limit" {
        return Err(reason);
    }
    reaped.map_err(|_| "wait")?;
    if observed_status
        .map_err(|_| "wait")?
        .is_some_and(|status| !status.success())
    {
        return Err("exit");
    }
    if Instant::now() >= deadline {
        return Err("timeout");
    }
    if sender.is_closed() {
        return Err("cancelled");
    }
    let diagnostics = reject_diagnostics(stderr.as_mut().ok_or("pipe")?);
    let checked = tokio::select! {
        result = tokio::time::timeout_at(deadline, diagnostics) => result.unwrap_or(Err("timeout")),
        _ = sender.closed() => Err("cancelled"),
    };
    match checked {
        Ok(()) => Err("output_limit"),
        Err(reason) => Err(reason),
    }
}

fn timed_out(started: Instant) -> Error {
    tracing::debug!(
        event = "media_command_failed",
        reason = "timeout",
        elapsed_ms = started.elapsed().as_millis() as u64,
        "Media command exceeded its deadline"
    );
    Error::Internal("Media inspection could not finish".into())
}

async fn read_output(stream: impl AsyncRead + Unpin, limit: usize) -> Outcome<Vec<u8>> {
    let mut output = Vec::new();
    stream
        .take(limit as u64 + 1)
        .read_to_end(&mut output)
        .await
        .map_err(|_| "read")?;
    if output.len() > limit {
        return Err("output_limit");
    }
    Ok(output)
}

async fn reject_diagnostics(mut stream: impl AsyncRead + Unpin) -> Outcome<()> {
    let mut byte = [0];
    if stream.read(&mut byte).await.map_err(|_| "read")? > 0 {
        return Err("diagnostic");
    }
    Ok(())
}
