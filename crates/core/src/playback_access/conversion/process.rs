use super::{ConversionSlot, StreamQuality};
use crate::{AudioFile, Error, MediaTools, PlaybackSource, Result, media_command};
use bytes::Bytes;
use std::{
    ffi::OsString,
    path::Path,
    process::Stdio,
    time::{Duration, Instant},
};
use tokio::{
    io::AsyncReadExt,
    process::{Child, ChildStdout},
    sync::mpsc,
};
use tracing::Instrument;

const CHUNK: usize = 32768;
const START_TIMEOUT: Duration = Duration::from_secs(10);
const READ_TIMEOUT: Duration = Duration::from_secs(15);
const EXIT_TIMEOUT: Duration = Duration::from_secs(2);

pub(super) fn stdin_of(file: &AudioFile) -> Result<Stdio> {
    file.file
        .try_clone()
        .map(Stdio::from)
        .map_err(|_| unfinished())
}

pub(super) fn unfinished() -> Error {
    Error::Internal("Audio conversion could not finish".into())
}

pub(super) fn spawn_error(error: std::io::Error) -> Error {
    tracing::warn!(event = "media_tool_unavailable", io_kind = ?error.kind(), "Audio conversion tool unavailable");
    match error.kind() {
        std::io::ErrorKind::NotFound | std::io::ErrorKind::PermissionDenied => {
            Error::MediaToolsMissing
        }
        _ => unfinished(),
    }
}

pub(super) async fn retire(child: &mut Child) {
    if !matches!(child.try_wait(), Ok(Some(_))) {
        let _ = child.kill().await;
    }
    let _ = child.wait().await;
}

pub(super) async fn start(
    tools: &MediaTools,
    file: &AudioFile,
    quality: StreamQuality,
    target: u32,
    start: Duration,
    authority: PlaybackSource,
    slot: ConversionSlot,
) -> Result<ConversionStream> {
    authority.check().await?;
    let mut arguments = media_command::audio_input_at(Path::new("/dev/stdin"), start);
    arguments.extend(
        [
            "-map",
            "0:a:0",
            "-vn",
            "-sn",
            "-dn",
            "-c:a",
            "aac",
            "-threads",
            "1",
            "-b:a",
            &target.to_string(),
            "-f",
            "adts",
            "pipe:1",
        ]
        .map(OsString::from),
    );
    let mut command = media_command::command(&tools.ffmpeg, arguments);
    command.stdin(stdin_of(file)?).stderr(Stdio::null());
    let mut child = command.spawn().map_err(spawn_error)?;
    let stdout = child.stdout.take();
    let (sender, receiver) = mpsc::channel(2);
    let owner = authority.clone();
    let span = tracing::debug_span!(
        "audio_conversion",
        playback_grant = authority.correlation_id(),
        quality = quality.label()
    );
    tokio::spawn(
        async move {
            let started = Instant::now();
            let mut bytes = 0;
            let (outcome, error) = tokio::select! {
                biased;
                _ = sender.closed() => ("cancelled", None),
                error = owner.revoked() => ("revoked", Some(error)),
                result = pump(&mut child, stdout, &sender, &mut bytes) => match result {
                    Ok(()) => ("complete", None),
                    Err(error) => ("failed", Some(error)),
                },
            };
            retire(&mut child).await;
            drop(slot);
            tracing::debug!(
                event = "audio_conversion_closed",
                outcome,
                bytes,
                elapsed_ms = started.elapsed().as_millis() as u64,
                "Audio conversion closed"
            );
            if let Some(error) = error {
                let _ = sender.send(Err(error)).await;
            }
        }
        .instrument(span),
    );
    let mut stream = ConversionStream {
        receiver,
        authority,
        first: None,
        finished: false,
    };
    let first = tokio::time::timeout(START_TIMEOUT, stream.chunk())
        .await
        .map_err(|_| unfinished())??;
    stream.first = Some(first.ok_or_else(unfinished)?);
    Ok(stream)
}

async fn pump(
    child: &mut Child,
    stdout: Option<ChildStdout>,
    sender: &mpsc::Sender<Result<Bytes>>,
    transferred: &mut u64,
) -> Result<()> {
    let mut stdout = stdout.ok_or_else(unfinished)?;
    loop {
        let mut bytes = vec![0; CHUNK];
        let count = tokio::time::timeout(READ_TIMEOUT, stdout.read(&mut bytes))
            .await
            .map_err(|_| unfinished())?
            .map_err(|_| unfinished())?;
        if count == 0 {
            let status = tokio::time::timeout(EXIT_TIMEOUT, child.wait())
                .await
                .map_err(|_| unfinished())?
                .map_err(|_| unfinished())?;
            return if status.success() {
                Ok(())
            } else {
                Err(unfinished())
            };
        }
        bytes.truncate(count);
        sender
            .send(Ok(Bytes::from(bytes)))
            .await
            .map_err(|_| unfinished())?;
        *transferred = transferred.saturating_add(count as u64);
    }
}

pub struct ConversionStream {
    receiver: mpsc::Receiver<Result<Bytes>>,
    authority: PlaybackSource,
    first: Option<Bytes>,
    finished: bool,
}

impl ConversionStream {
    #[tracing::instrument(level = "trace", skip_all)]
    pub async fn chunk(&mut self) -> Result<Option<Bytes>> {
        if self.finished {
            return Ok(None);
        }
        let first = &mut self.first;
        let receiver = &mut self.receiver;
        let result = self
            .authority
            .while_authorized(async {
                if let Some(bytes) = first.take() {
                    return Ok(Some(bytes));
                }
                receiver.recv().await.transpose()
            })
            .await;
        if !matches!(result, Ok(Some(_))) {
            self.finished = true;
            self.first = None;
            self.receiver.close();
        }
        result
    }
}
