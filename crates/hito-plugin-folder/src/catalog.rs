use cap_fs_ext::{FollowSymlinks, OpenOptionsFollowExt};
use cap_std::fs::{Dir, OpenOptions};
use hito_plugin_sdk::{ProviderRecording, ProviderRelease};
use std::{
    collections::BTreeMap,
    ffi::OsString,
    fs::{File, Metadata},
    io,
    sync::Arc,
    time::SystemTime,
};

pub(crate) struct Catalog {
    pub tracks: BTreeMap<String, Track>,
    pub fingerprint: String,
    pub claim: Option<Arc<crate::registry::Claim>>,
}
pub(crate) struct Track {
    pub id: String,
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
            revision: Some(self.id.clone()),
        }
    }
    pub fn source_id(&self) -> String {
        format!("source-{}", self.id)
    }
    pub fn open(&self) -> io::Result<File> {
        let file = open(&self.directory, &self.name)?;
        if Stamp::read(&file.metadata()?)? != self.stamp {
            return Err(io::Error::other(
                "Audio changed; restart the folder service",
            ));
        }
        Ok(file)
    }
}
#[derive(PartialEq, Eq)]
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
