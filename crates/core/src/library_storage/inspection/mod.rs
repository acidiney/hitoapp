mod artist_tags;
mod cache;
mod command;
mod ownership;
mod probe;
mod read_probe;
mod snapshot;
mod tagged_artists;

use super::{
    root::Root,
    types::{AUDIO_ALGORITHM, AudioEvidence, IndexedFile},
};
use crate::{Error, MediaTools, Result};
pub(crate) use cache::CachedFile;
use command::Commands;
pub(super) use ownership::Scope;
use probe::Probe;
use sha2::{Digest, Sha256};
use std::{ffi::OsString, path::Path, time::Duration};

#[derive(Clone)]
pub(crate) struct FileInspector {
    tools: MediaTools,
    commands: Commands,
    snapshots: snapshot::Reader,
}

use crate::media_command::audio_input as input;

enum AudioInspection {
    Verified(AudioEvidence),
    UnsupportedProfile,
}

impl FileInspector {
    pub(super) async fn settle(&self) -> Result<()> {
        self.snapshots.settle().await?;
        self.commands.settle().await
    }

    pub(super) fn parallelism(&self) -> usize {
        self.tools.parallelism.clamp(1, 64)
    }

    pub(super) fn scoped(&self, retirement: Scope) -> Self {
        Self {
            tools: self.tools.clone(),
            commands: self.commands.scoped(retirement.clone()),
            snapshots: self.snapshots.scoped(retirement),
        }
    }

    pub(crate) fn new(tools: MediaTools) -> Self {
        let commands = Commands::new(tools.parallelism);
        let snapshots = snapshot::Reader::new(tools.temp_budget, tools.temp_dir.clone());
        Self {
            tools,
            commands,
            snapshots,
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
        Ok(self.inspect_media(root, path, cached).await?.0)
    }

    pub(super) async fn inspect_download(
        &self,
        root: &Root,
        path: &Path,
    ) -> Result<(IndexedFile, &'static str)> {
        let (file, mime) = self.inspect_media(root, path, None).await?;
        if file.audio.is_none() {
            return Err(Error::Invalid(
                "This audio profile cannot be verified for download".into(),
            ));
        }
        Ok((
            file,
            mime.ok_or_else(|| Error::Invalid("This audio format cannot be downloaded".into()))?,
        ))
    }

    async fn inspect_media(
        &self,
        root: &Root,
        path: &Path,
        cached: Option<CachedFile>,
    ) -> Result<(IndexedFile, Option<&'static str>)> {
        let prepared = self
            .snapshots
            .prepare(
                root,
                path,
                cached
                    .as_ref()
                    .filter(|cached| cached.metadata_current())
                    .map(|cached| cached.checksum.as_str()),
                self.tools.timeout,
            )
            .await?;
        let before = prepared.file;
        let cached = match cached.filter(|cached| cached.checksum == before.checksum) {
            Some(cached) if cached.metadata_current() => {
                tracing::trace!(
                    event = "media_inspection_reused",
                    "Unchanged file inspection reused"
                );
                return Ok((cached.into_indexed(before), None));
            }
            cached => cached,
        };
        let media = prepared
            .media
            .ok_or_else(|| Error::Internal("Media verification did not prepare an input".into()))?;
        let (probe, tags) = self.read_probe(&media).await?;
        if probe.streams.is_empty() {
            return Err(Error::Invalid("No audio stream found".into()));
        }
        let audio = match cached {
            Some(cached) => {
                tracing::debug!(
                    event = "audio_inspection_reused",
                    metadata_version = cached.metadata_version,
                    "Unchanged audio inspection reused for metadata refresh"
                );
                cached.audio()
            }
            None => match self.audio(&media, &probe).await? {
                AudioInspection::Verified(evidence) => Some(evidence),
                AudioInspection::UnsupportedProfile => None,
            },
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
        let credits = tagged_artists::credits(&artist, media.artists.as_ref());
        Ok((
            IndexedFile {
                credits,
                path: before.path,
                checksum: before.checksum,
                size: before.size,
                title,
                artist,
                album_artist,
                album: probe.tag("album").unwrap_or_else(|| "Unknown album".into()),
                duration_ms: probe.duration_ms(),
                disc_number: probe.number("disc"),
                track_number: probe.number("track"),
                tag_details: Some(
                    serde_json::to_string(&tags).map_err(|_| {
                        Error::Internal("Observed tags could not be encoded".into())
                    })?,
                ),
                audio,
            },
            probe.mime(),
        ))
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
