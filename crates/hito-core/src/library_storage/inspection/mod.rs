mod cache;
mod command;
mod probe;
mod snapshot;

use super::{
    root::Root,
    types::{AUDIO_ALGORITHM, AudioEvidence, IndexedFile},
};
use crate::{Error, MediaTools, Result};
pub(crate) use cache::CachedFile;
use command::Commands;
use probe::Probe;
use sha2::{Digest, Sha256};
use std::{ffi::OsString, path::Path, time::Duration};

#[derive(Clone)]
pub(crate) struct FileInspector {
    tools: MediaTools,
    commands: Commands,
    snapshots: snapshot::Reader,
}

const FORMATS: &str = "aac,aiff,asf,flac,mp3,mov,ogg,wav";

enum AudioInspection {
    Verified(AudioEvidence),
    UnsupportedProfile,
}

impl FileInspector {
    pub(crate) fn new(tools: MediaTools) -> Self {
        Self {
            tools,
            commands: Commands::new(),
            snapshots: snapshot::Reader::new(),
        }
    }

    pub(super) async fn check_tools(&self) -> Result<()> {
        for tool in [&self.tools.ffprobe, &self.tools.ffmpeg] {
            self.commands
                .run(
                    tool,
                    vec!["-version".into()],
                    65536,
                    self.tools.timeout.min(Duration::from_secs(15)),
                    None,
                )
                .await?;
        }
        Ok(())
    }

    #[tracing::instrument(level = "debug", skip_all, err(Debug))]
    pub(super) async fn inspect(
        &self,
        root: &Root,
        path: &Path,
        cached: Option<CachedFile>,
    ) -> Result<IndexedFile> {
        let prepared = self
            .snapshots
            .prepare(
                root,
                path,
                cached.as_ref().map(|cached| cached.checksum.as_str()),
                self.tools.timeout,
            )
            .await?;
        let before = prepared.file;
        if let Some(cached) = cached.filter(|cached| cached.checksum == before.checksum) {
            tracing::trace!(
                event = "media_inspection_reused",
                "Unchanged file inspection reused"
            );
            return Ok(cached.into_indexed(before));
        }
        let media = prepared
            .media
            .ok_or_else(|| Error::Internal("Media verification did not prepare an input".into()))?;
        let mut arguments = input(&media.path);
        arguments.extend([
            "-select_streams", "a", "-show_entries",
            "stream=codec_name,sample_rate,channels,channel_layout,bits_per_sample,bits_per_raw_sample:format=duration:format_tags=title,artist,album,album_artist:stream_tags=title,artist,album,album_artist",
            "-of", "json",
        ].map(OsString::from));
        let bytes = self
            .commands
            .run(
                &self.tools.ffprobe,
                arguments,
                65536,
                self.tools.timeout.min(Duration::from_secs(15)),
                Some(media.directory.clone()),
            )
            .await?;
        let probe: Probe = serde_json::from_slice(&bytes)
            .map_err(|_| Error::Invalid("Media inspection returned invalid metadata".into()))?;
        if probe.streams.is_empty() {
            return Err(Error::Invalid("No audio stream found".into()));
        }
        let audio = match self.audio(&media, &probe).await? {
            AudioInspection::Verified(evidence) => Some(evidence),
            AudioInspection::UnsupportedProfile => None,
        };
        let after = self
            .snapshots
            .checksum(root, &before.path, self.tools.timeout)
            .await?;
        if before != after {
            return Err(Error::Conflict(
                "Media changed during inspection; scan again".into(),
            ));
        }
        let title = probe.tag("title").unwrap_or_else(|| {
            before
                .path
                .file_stem()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned()
        });
        let artist = probe
            .tag("artist")
            .unwrap_or_else(|| "Unknown artist".into());
        let album_artist = probe.tag("album_artist").unwrap_or_else(|| artist.clone());
        Ok(IndexedFile {
            path: before.path,
            checksum: before.checksum,
            size: before.size,
            title,
            artist,
            album_artist,
            album: probe.tag("album").unwrap_or_else(|| "Unknown album".into()),
            duration_ms: probe.duration_ms(),
            audio,
        })
    }

    pub(super) async fn checksum(&self, root: &Root, path: &Path) -> Result<String> {
        Ok(self
            .snapshots
            .checksum(root, path, self.tools.timeout)
            .await?
            .checksum)
    }

    async fn audio(
        &self,
        media: &snapshot::MediaSnapshot,
        probe: &Probe,
    ) -> Result<AudioInspection> {
        let Some(profile) = probe.profile() else {
            tracing::debug!(reason = "unsupported_profile", "Audio evidence unavailable");
            return Ok(AudioInspection::UnsupportedProfile);
        };
        let mut arguments = input(&media.path);
        arguments.extend(["-nostdin", "-xerror"].map(OsString::from));
        arguments.extend(
            [
                "-map",
                "0:a:0",
                "-vn",
                "-sn",
                "-dn",
                "-threads",
                "1",
                "-c:a",
                "pcm_f64le",
                "-flags",
                "+bitexact",
                "-fflags",
                "+bitexact",
                "-f",
                "hash",
                "-hash",
                "sha256",
                "-",
            ]
            .map(OsString::from),
        );
        let output = self
            .commands
            .run(
                &self.tools.ffmpeg,
                arguments,
                256,
                self.tools.timeout,
                Some(media.directory.clone()),
            )
            .await?;
        let hash = std::str::from_utf8(&output)
            .ok()
            .and_then(|value| value.trim().strip_prefix("SHA256="))
            .ok_or_else(|| {
                Error::Invalid("Media inspection returned invalid audio evidence".into())
            })?;
        if hash.len() != 64
            || !hash.bytes().all(|v| v.is_ascii_hexdigit())
            || hash == format!("{:x}", Sha256::digest([]))
        {
            return Err(Error::Invalid(
                "Media inspection returned invalid audio evidence".into(),
            ));
        }
        let descriptor = serde_json::to_vec(&(AUDIO_ALGORITHM, profile, hash))
            .map_err(|_| Error::Internal("Audio evidence could not be encoded".into()))?;
        tracing::debug!(event = "audio_evidence_verified", "Audio evidence verified");
        Ok(AudioInspection::Verified(AudioEvidence {
            algorithm: AUDIO_ALGORITHM,
            digest: format!("{:x}", Sha256::digest(descriptor)),
        }))
    }
}

fn input(path: &Path) -> Vec<OsString> {
    let mut arguments: Vec<_> = [
        "-v",
        "error",
        "-max_alloc",
        "16777216",
        "-threads",
        "1",
        "-protocol_whitelist",
        "file,pipe",
        "-format_whitelist",
        FORMATS,
        "-probesize",
        "8388608",
        "-analyzeduration",
        "10000000",
        "-i",
    ]
    .map(OsString::from)
    .into();
    arguments.push(path.as_os_str().to_owned());
    arguments
}
