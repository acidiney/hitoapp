use super::{Message, PcmBlock, PcmFormat, PcmStream, opening::Stop, unfinished};
use crate::playback_access::conversion::ConversionSlot;
use crate::{AudioFile, Error, MediaTools, PlaybackSource, Result, media_command};
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
use tracing::{Instrument, instrument::WithSubscriber};

pub(super) fn spawn_error(error: std::io::Error) -> Error {
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
    file: AudioFile,
    format: PcmFormat,
    start: Duration,
    authority: PlaybackSource,
    slot: ConversionSlot,
    stop: Stop,
) -> Result<PcmStream> {
    authority.check().await?;
    if start > Duration::ZERO && format.duration.is_some_and(|duration| start >= duration) {
        let (_, receiver) = mpsc::channel(1);
        tracing::debug!(
            event = "native_pcm_end",
            outcome = "complete",
            "Native decoder opened at the known end"
        );
        return Ok(PcmStream {
            format,
            authority,
            receiver,
            pending: Some(Message::End),
            cancellation: stop,
            owner: None,
            finished: false,
        });
    }
    let first_frame =
        ((start.as_nanos() * u128::from(format.sample_rate) + 500_000_000) / 1_000_000_000) as u64;
    let effective_start = first_frame as f64 / f64::from(format.sample_rate);
    let preroll = effective_start.min(1.0);
    let mut arguments = media_command::audio_input(Path::new("/dev/stdin"));
    let input = arguments.len() - 2;
    arguments.splice(
        input..input,
        [
            "-ss".into(),
            format!("{:.9}", effective_start - preroll).into(),
        ],
    );
    arguments.extend(["-ss".into(), format!("{preroll:.9}").into()]);
    arguments.extend(
        [
            "-map",
            "0:a:0",
            "-vn",
            "-sn",
            "-dn",
            "-c:a",
            "pcm_f32le",
            "-threads",
            "1",
            "-f",
            "f32le",
        ]
        .map(OsString::from),
    );
    arguments.extend([
        "-ar".into(),
        format.sample_rate.to_string().into(),
        "-ac".into(),
        format.channels.to_string().into(),
    ]);
    arguments.push("pipe:1".into());
    let mut command = media_command::command(&tools.ffmpeg, arguments);
    command.stdin(Stdio::from(file.file)).stderr(Stdio::null());
    let mut child = command.spawn().map_err(spawn_error)?;
    let stdout = child.stdout.take();
    let (sender, receiver) = mpsc::channel(2);
    let owner_stop = stop.clone();
    let owner_authority = authority.clone();
    let span = tracing::debug_span!(
        "native_pcm_decoder",
        playback_grant = authority.correlation_id(),
        account_id = authority.actor().id()
    );
    let owner = tokio::spawn(async move {
        let started = Instant::now();
        let mut samples = 0;
        let result = tokio::select! {
            biased;
            _ = owner_stop.cancelled() => None,
            _ = sender.closed() => None,
            error = owner_authority.revoked() => Some(Err(error)),
            result = pump(&mut child, stdout, &sender, format, first_frame, &mut samples) => Some(result),
        };
        retire(&mut child).await;
        drop(slot);
        let outcome = match &result { None => "cancelled", Some(Ok(())) => "complete", Some(Err(_)) => "failed" };
        let error_kind = result.as_ref().and_then(|result| result.as_ref().err()).map(Error::code);
        tracing::debug!(event = "native_pcm_closed", outcome, error_kind, samples, elapsed_ms = started.elapsed().as_millis() as u64, "Native decoder closed");
        if let Some(result) = result {
            let terminal = match result { Ok(()) => Message::End, Err(error) => Message::Failed(error) };
            let _ = sender.send(terminal).await;
        }
    }.instrument(span).with_current_subscriber());
    let mut stream = PcmStream {
        format,
        authority,
        receiver,
        pending: None,
        cancellation: stop.clone(),
        owner: Some(owner),
        finished: false,
    };
    let first = tokio::select! {
        biased;
        _ = stop.cancelled() => Ok(Err(super::opening::cancelled())),
        first = tokio::time::timeout(Duration::from_secs(10), stream.next_block()) => first,
    };
    match first {
        Ok(Ok(Some(block))) => {
            stream.pending = Some(Message::Block(block));
            Ok(stream)
        }
        result => {
            let error = match result {
                Ok(Err(error)) => error,
                _ => unfinished(),
            };
            stream.close().await?;
            Err(error)
        }
    }
}

async fn pump(
    child: &mut Child,
    stdout: Option<ChildStdout>,
    sender: &mpsc::Sender<Message>,
    format: PcmFormat,
    first_frame: u64,
    transferred: &mut u64,
) -> Result<()> {
    let mut stdout = stdout.ok_or_else(unfinished)?;
    let frame_bytes = usize::from(format.channels) * 4;
    let mut buffer = vec![0; 32_768];
    let mut retained = 0;
    loop {
        let count = tokio::time::timeout(
            Duration::from_secs(15),
            stdout.read(&mut buffer[retained..]),
        )
        .await
        .map_err(|_| unfinished())?
        .map_err(|_| unfinished())?;
        if count == 0 {
            if retained != 0 {
                return Err(unfinished());
            }
            let status = tokio::time::timeout(Duration::from_secs(2), child.wait())
                .await
                .map_err(|_| unfinished())?
                .map_err(|_| unfinished())?;
            return if status.success() {
                Ok(())
            } else {
                Err(unfinished())
            };
        }
        let available = retained + count;
        let aligned = available / frame_bytes * frame_bytes;
        if aligned > 0 {
            let samples: Vec<f32> = buffer[..aligned]
                .as_chunks::<4>()
                .0
                .iter()
                .map(|bytes| f32::from_le_bytes(*bytes))
                .collect();
            if samples.iter().any(|sample| !sample.is_finite()) {
                return Err(unfinished());
            }
            let frame = first_frame + *transferred / u64::from(format.channels);
            let position = Duration::from_secs_f64(frame as f64 / f64::from(format.sample_rate));
            *transferred = transferred.saturating_add(samples.len() as u64);
            sender
                .send(Message::Block(PcmBlock { samples, position }))
                .await
                .map_err(|_| unfinished())?;
        }
        buffer.copy_within(aligned..available, 0);
        retained = available - aligned;
    }
}
