use super::{ConversionSlot, QualityRequest};
use crate::{AudioFile, Error, MediaTools, PlaybackSource, Result, media_command};
use bytes::Bytes;
use std::sync::Arc;
use std::{
    ffi::OsString,
    path::Path,
    process::Stdio,
    time::{Duration, Instant},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    process::{Child, ChildStdin, ChildStdout},
    sync::{mpsc, watch},
    task::JoinHandle,
};
use tracing::Instrument;

const CHUNK: usize = 32768;
const START_TIMEOUT: Duration = Duration::from_secs(10);
const READ_TIMEOUT: Duration = Duration::from_secs(15);
const EXIT_TIMEOUT: Duration = Duration::from_secs(2);

pub(crate) trait PositionedAudio: Send + Sync + 'static {
    fn read_at(&self, offset: u64, count: usize) -> Result<Bytes>;
}

#[derive(Clone, Copy)]
pub(crate) enum Input<'a> {
    Descriptor(&'a AudioFile),
    Positioned(&'a Arc<dyn PositionedAudio>),
}

impl Input<'_> {
    pub(super) fn stdin(&self) -> Result<Stdio> {
        match self {
            Self::Descriptor(file) => file
                .file
                .try_clone()
                .map(Stdio::from)
                .map_err(|_| unfinished()),
            Self::Positioned(_) => Ok(Stdio::piped()),
        }
    }

    pub(super) fn feeder(&self, child: &mut Child) -> Option<Feeder> {
        match self {
            Self::Descriptor(_) => None,
            Self::Positioned(source) => child.stdin.take().map(|stdin| Feeder {
                stdin,
                source: (*source).clone(),
            }),
        }
    }
}

pub(super) struct Feeder {
    stdin: ChildStdin,
    source: Arc<dyn PositionedAudio>,
}

impl Feeder {
    async fn run(mut self, mut stop: watch::Receiver<bool>) -> Result<()> {
        let mut offset = 0;
        loop {
            if *stop.borrow() {
                return Ok(());
            }
            let source = self.source.clone();
            let bytes = tokio::task::spawn_blocking(move || source.read_at(offset, CHUNK))
                .await
                .map_err(|_| unfinished())??;
            if bytes.is_empty() {
                return Ok(());
            }
            offset += bytes.len() as u64;
            tokio::select! {
                biased;
                _ = stop.changed() => return Ok(()),
                written = self.stdin.write_all(&bytes) => if written.is_err() { return Ok(()); },
            }
        }
    }
}

pub(super) async fn fed<T, Work>(
    feeder: Option<Feeder>,
    work: impl FnOnce(watch::Sender<bool>) -> Work,
) -> (T, Result<()>)
where
    Work: Future<Output = T>,
{
    let (stop, stopped) = watch::channel(false);
    let feeding = async move {
        match feeder {
            Some(feeder) => feeder.run(stopped).await,
            None => Ok(()),
        }
    };
    tokio::join!(work(stop), feeding)
}

pub(crate) async fn cancelled(cancel: &mut watch::Receiver<bool>) {
    if cancel.wait_for(|stop| *stop).await.is_err() {
        std::future::pending::<()>().await;
    }
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
    input: Input<'_>,
    request: QualityRequest,
    authority: PlaybackSource,
    slot: ConversionSlot,
    cancel: &mut watch::Receiver<bool>,
) -> Result<ConversionStream> {
    let quality = request.quality;
    let target = quality.target_bitrate().ok_or_else(unfinished)?;
    authority.check().await?;
    if *cancel.borrow() {
        return Err(unfinished());
    }
    let mut arguments = media_command::audio_input_at(Path::new("/dev/stdin"), request.start);
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
    command.stdin(input.stdin()?).stderr(Stdio::null());
    let mut child = command.spawn().map_err(spawn_error)?;
    let stdout = child.stdout.take();
    let feeder = input.feeder(&mut child);
    let (sender, receiver) = mpsc::channel(2);
    let owner = authority.clone();
    let span = tracing::debug_span!(
        "audio_conversion",
        playback_grant = authority.correlation_id(),
        quality = quality.label()
    );
    let task = tokio::spawn(
        async move {
            let started = Instant::now();
            let mut bytes = 0;
            let (child, sender, owner, transferred) = (&mut child, &sender, &owner, &mut bytes);
            let ((outcome, error), fed) = fed(feeder, |stop| async move {
                let outcome = tokio::select! {
                    biased;
                    _ = sender.closed() => ("cancelled", None),
                    error = owner.revoked() => ("revoked", Some(error)),
                    result = pump(child, stdout, sender, transferred) => match result {
                        Ok(()) => ("complete", None),
                        Err(error) => ("failed", Some(error)),
                    },
                };
                let _ = stop.send(true);
                retire(child).await;
                outcome
            })
            .await;
            let (outcome, error) = match (outcome, error, fed) {
                ("complete", None, Err(error)) => ("failed", Some(error)),
                (outcome, error, _) => (outcome, error),
            };
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
        task: Some(task),
    };
    let first = tokio::select! {
        biased;
        _ = cancelled(cancel) => Err(unfinished()),
        first = tokio::time::timeout(START_TIMEOUT, stream.chunk()) => first
            .map_err(|_| unfinished())
            .and_then(|first| first)
            .and_then(|first| first.ok_or_else(unfinished)),
    };
    match first {
        Ok(first) => {
            stream.first = Some(first);
            Ok(stream)
        }
        Err(error) => {
            stream.close().await;
            Err(error)
        }
    }
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
    task: Option<JoinHandle<()>>,
}

impl ConversionStream {
    pub(crate) async fn close(mut self) {
        self.finished = true;
        self.first = None;
        self.receiver.close();
        while self.receiver.recv().await.is_some() {}
        if let Some(task) = self.task.take() {
            let _ = task.await;
        }
    }

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
