use super::{PcmFormat, opening::Stop, unfinished};
use crate::playback_access::conversion::ConversionSlot;
use crate::{AudioFile, Error, MediaTools, PlaybackSource, Result, media_command};
use serde::Deserialize;
use std::{ffi::OsString, path::Path, process::Stdio, time::Duration};
use tokio::io::AsyncReadExt;

#[derive(Deserialize)]
struct Probe {
    streams: Vec<Stream>,
    format: Option<Container>,
}

#[derive(Deserialize)]
struct Stream {
    codec_name: Option<String>,
    sample_rate: Option<String>,
    channels: Option<u16>,
    duration: Option<String>,
}

#[derive(Deserialize)]
struct Container {
    duration: Option<String>,
}

#[tracing::instrument(level = "debug", skip_all, fields(playback_grant = authority.correlation_id(), account_id = authority.actor().id()))]
pub(super) async fn probe(
    tools: &MediaTools,
    file: &AudioFile,
    authority: PlaybackSource,
    slot: ConversionSlot,
    stop: &Stop,
) -> Result<(PcmFormat, ConversionSlot)> {
    authority.check().await?;
    let mut arguments = media_command::audio_input(Path::new("/dev/stdin"));
    arguments.extend(
        [
            "-select_streams",
            "a:0",
            "-show_entries",
            "stream=codec_name,sample_rate,channels,duration:format=duration",
            "-of",
            "json",
        ]
        .map(OsString::from),
    );
    let mut command = media_command::command(&tools.ffprobe, arguments);
    command
        .stdin(Stdio::from(
            file.file.try_clone().map_err(|_| unfinished())?,
        ))
        .stderr(Stdio::null());
    let mut child = command.spawn().map_err(super::process::spawn_error)?;
    let stdout = child.stdout.take();
    let inspect = async {
        let mut output = Vec::new();
        stdout
            .ok_or_else(unfinished)?
            .take(16_385)
            .read_to_end(&mut output)
            .await
            .map_err(|_| unfinished())?;
        if output.len() > 16_384 || !child.wait().await.map_err(|_| unfinished())?.success() {
            return Err(unfinished());
        }
        parse(&output)
    };
    let result = tokio::select! {
            biased;
            _ = stop.cancelled() => Err(super::opening::cancelled()),
            error = authority.revoked() => Err(error),
            result = tokio::time::timeout(Duration::from_secs(10), inspect) => result.unwrap_or_else(|_| Err(unfinished())),
    };
    super::process::retire(&mut child).await;
    result.map(|format| (format, slot))
}

fn parse(bytes: &[u8]) -> Result<PcmFormat> {
    let mut probe: Probe = serde_json::from_slice(bytes).map_err(|_| unfinished())?;
    let stream = probe.streams.pop().ok_or_else(unfinished)?;
    let rate: u32 = stream
        .sample_rate
        .and_then(|rate| rate.parse().ok())
        .ok_or_else(unfinished)?;
    let channels = stream.channels.ok_or_else(unfinished)?;
    if stream.codec_name.as_deref() != Some("opus")
        || !(8_000..=192_000).contains(&rate)
        || !(1..=2).contains(&channels)
    {
        return Err(Error::UnsupportedAudio);
    }
    let duration = stream
        .duration
        .or_else(|| probe.format.and_then(|format| format.duration))
        .and_then(|duration| duration.parse::<f64>().ok())
        .and_then(|duration| Duration::try_from_secs_f64(duration).ok());
    Ok(PcmFormat {
        sample_rate: rate,
        channels,
        duration,
    })
}
