use super::{
    ConversionSlot,
    process::{retire, spawn_error, stdin_of, unfinished},
};
use crate::{AudioFile, MediaTools, PlaybackSource, Result, media_command};
use serde::Deserialize;
use std::{ffi::OsString, path::Path, process::Stdio, time::Duration};
use tokio::{io::AsyncReadExt, sync::oneshot};
use tracing::Instrument;

const PROBE_LIMIT: usize = 16384;
const PROBE_TIMEOUT: Duration = Duration::from_secs(10);
const LOSSY: &[&str] = &[
    "mp3", "mp2", "aac", "vorbis", "opus", "ac3", "eac3", "wmav1", "wmav2",
];
const LOSSLESS: &[&str] = &[
    "flac",
    "alac",
    "wavpack",
    "ape",
    "tta",
    "pcm_s16le",
    "pcm_s16be",
    "pcm_s24le",
    "pcm_s24be",
    "pcm_s32le",
    "pcm_s32be",
    "pcm_f32le",
    "pcm_f32be",
    "pcm_f64le",
    "pcm_f64be",
    "pcm_u8",
    "pcm_s8",
];

#[derive(Deserialize, Default)]
struct Probed {
    #[serde(default)]
    streams: Vec<ProbedStream>,
}
#[derive(Deserialize, Default)]
struct ProbedStream {
    codec_name: Option<String>,
    bit_rate: Option<String>,
}
pub(super) struct SourceProfile {
    codec: Option<String>,
    bits_per_second: Option<u32>,
}
impl SourceProfile {
    pub(super) fn passthrough_reason(&self, target: u32) -> Option<&'static str> {
        match self.codec.as_deref() {
            Some(codec) if LOSSLESS.contains(&codec) => None,
            Some(codec) if LOSSY.contains(&codec) => match self.bits_per_second {
                Some(rate) if rate > target => None,
                Some(rate) if rate > 0 => Some("lossy-within-target"),
                _ => Some("uncertain-profile"),
            },
            _ => Some("uncertain-profile"),
        }
    }
}

pub(super) async fn probe(
    tools: &MediaTools,
    file: &AudioFile,
    authority: PlaybackSource,
    slot: ConversionSlot,
) -> Result<(SourceProfile, ConversionSlot)> {
    authority.check().await?;
    let mut arguments = media_command::audio_input(Path::new("/dev/stdin"));
    arguments.extend(
        [
            "-select_streams",
            "a:0",
            "-show_entries",
            "stream=codec_name,bit_rate:format=bit_rate",
            "-of",
            "json",
        ]
        .map(OsString::from),
    );
    let mut command = media_command::command(&tools.ffprobe, arguments);
    command.stdin(stdin_of(file)?).stderr(Stdio::null());
    let mut child = command.spawn().map_err(spawn_error)?;
    let stdout = child.stdout.take();
    let (mut sender, receiver) = oneshot::channel();
    let span = tracing::debug_span!(
        "audio_conversion_probe",
        playback_grant = authority.correlation_id()
    );
    tokio::spawn(async move {
        let operation = async {
            let mut output = Vec::new();
            stdout.ok_or_else(unfinished)?.take(PROBE_LIMIT as u64 + 1).read_to_end(&mut output).await.map_err(|_| unfinished())?;
            if output.len() > PROBE_LIMIT { return Err(unfinished()); }
            if !child.wait().await.map_err(|_| unfinished())?.success() { return Err(unfinished()); }
            parse(&output)
        };
        let result = tokio::select! {
            biased;
            _ = sender.closed() => Err(unfinished()),
            error = authority.revoked() => Err(error),
            result = tokio::time::timeout(PROBE_TIMEOUT, operation) => result.unwrap_or_else(|_| Err(unfinished())),
        };
        retire(&mut child).await;
        let _ = sender.send(result.map(|profile| (profile, slot)));
    }.instrument(span));
    receiver.await.map_err(|_| unfinished())?
}

fn parse(output: &[u8]) -> Result<SourceProfile> {
    let probed: Probed = serde_json::from_slice(output).map_err(|_| unfinished())?;
    let stream = probed.streams.into_iter().next().ok_or_else(unfinished)?;
    Ok(SourceProfile {
        codec: stream.codec_name,
        bits_per_second: stream.bit_rate.and_then(|value| value.parse().ok()),
    })
}
