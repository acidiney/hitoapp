use crate::representation::{AudioFile, ChunkDigest};
use cap_fs_ext::{FollowSymlinks, OpenOptionsFollowExt};
use cap_std::fs::{Dir, OpenOptions};
use hito_plugin_sdk::{ProviderRecording, ProviderRelease};
use std::{
    collections::BTreeMap,
    ffi::OsString,
    fs::{File, Metadata},
    io,
    sync::Arc,
    time::{Instant, SystemTime},
};

pub(crate) struct Catalog {
    pub tracks: BTreeMap<String, Track>,
    pub retained: BTreeMap<String, ProviderRecording>,
    pub observations: Vec<Track>,
    pub sources: BTreeMap<String, CurrentSource>,
    pub recording_sources: BTreeMap<String, Vec<String>>,
    pub fingerprint: String,
    pub claim: Option<Arc<crate::registry::Claim>>,
}
pub(crate) struct CurrentSource {
    recording: String,
    checksum: String,
    locations: Vec<usize>,
}
pub(crate) struct SourceFile {
    pub file: AudioFile,
    pub length: u64,
    pub mime: &'static str,
}
impl Catalog {
    pub fn index_sources(&mut self) -> io::Result<()> {
        let mut sources = BTreeMap::<String, CurrentSource>::new();
        let mut recordings = BTreeMap::<String, Vec<String>>::new();
        for (index, track) in self.observations.iter().enumerate() {
            let source = sources
                .entry(track.source.clone())
                .or_insert_with(|| CurrentSource {
                    recording: track.id.clone(),
                    checksum: track.checksum.clone(),
                    locations: Vec::new(),
                });
            if source.recording != track.id || source.checksum != track.checksum {
                return Err(io::Error::other("Folder source binding changed"));
            }
            if source.locations.is_empty() {
                let ids = recordings.entry(track.id.clone()).or_default();
                ids.push(track.source.clone());
                if ids.len() > hito_plugin_sdk::MAX_SOURCES_PER_RECORDING {
                    return Err(io::Error::other("Folder current source limit exceeded"));
                }
            }
            source.locations.push(index);
        }
        self.sources = sources;
        self.recording_sources = recordings;
        Ok(())
    }
    pub fn open_source(&self, id: &str, deadline: Instant) -> io::Result<Option<SourceFile>> {
        let Some(source) = self.sources.get(id) else {
            return Ok(None);
        };
        for index in &source.locations {
            crate::inspection::check(deadline)?;
            let track = &self.observations[*index];
            let opened = track.open();
            crate::inspection::check(deadline)?;
            if let Ok(file) = opened {
                return Ok(Some(SourceFile {
                    file,
                    length: track.stamp.size,
                    mime: track.mime,
                }));
            }
        }
        Ok(None)
    }
}
#[derive(Clone)]
pub(crate) struct Track {
    pub id: String,
    pub checksum: String,
    pub chunks: Arc<[ChunkDigest]>,
    pub evidence: Option<String>,
    pub source: String,
    pub revision: String,
    pub locator: Vec<u8>,
    pub title: String,
    pub searchable: String,
    pub artist: String,
    pub album: Option<String>,
    pub release_id: Option<String>,
    pub album_artist: String,
    pub duration_ms: u64,
    pub track_number: Option<u32>,
    pub disc_number: Option<u32>,
    pub mime: &'static str,
    pub directory: Arc<Dir>,
    pub name: OsString,
    pub stamp: Stamp,
}
impl Track {
    pub fn recording(&self) -> ProviderRecording {
        ProviderRecording {
            id: self.id.clone(),
            title: self.title.clone(),
            artist: self.artist.clone(),
            duration_ms: Some(self.duration_ms),
            release: self
                .album
                .as_ref()
                .zip(self.release_id.as_ref())
                .map(|(title, id)| ProviderRelease {
                    id: id.clone(),
                    title: title.clone(),
                    artist: self.album_artist.clone(),
                }),
            disc_number: self.disc_number,
            track_number: self.track_number,
            revision: Some(self.revision.clone()),
        }
    }
    pub fn open(&self) -> io::Result<AudioFile> {
        AudioFile::new(
            open(&self.directory, &self.name)?,
            self.stamp.clone(),
            self.chunks.clone(),
        )
    }
}
#[derive(Clone, PartialEq, Eq)]
pub(crate) struct Stamp {
    pub size: u64,
    modified: SystemTime,
    #[cfg(unix)]
    identity: (u64, u64, i64, i64),
}
impl Stamp {
    pub fn read(metadata: &Metadata) -> io::Result<Self> {
        if !metadata.is_file() {
            return Err(io::Error::other("Audio requires a regular file"));
        }
        Ok(Self {
            size: metadata.len(),
            modified: metadata.modified()?,
            #[cfg(unix)]
            identity: {
                use std::os::unix::fs::MetadataExt;
                (
                    metadata.dev(),
                    metadata.ino(),
                    metadata.ctime(),
                    metadata.ctime_nsec(),
                )
            },
        })
    }
}
pub(crate) fn open(directory: &Dir, name: &OsString) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options.read(true).follow(FollowSymlinks::No);
    #[cfg(unix)]
    {
        use cap_std::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NONBLOCK);
    }
    let file = directory.open_with(name, &options)?.into_std();
    if !file.metadata()?.is_file() {
        return Err(io::Error::other("Audio requires a regular file"));
    }
    Ok(file)
}
