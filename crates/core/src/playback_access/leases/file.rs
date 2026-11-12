use super::{Entry, owner::BodyControl};
use crate::{AudioFile, Error, Result};
use bytes::Bytes;
use std::{
    fs::{File, Metadata},
    sync::Arc,
    time::SystemTime,
};

pub(super) struct RetainedFile {
    file: File,
    stamp: Stamp,
    pub length: u64,
    pub content_type: &'static str,
}

#[derive(PartialEq)]
struct Stamp {
    size: u64,
    modified: SystemTime,
    #[cfg(unix)]
    identity: (u64, u64, i64, i64),
    #[cfg(windows)]
    created: u64,
}

impl Stamp {
    fn read(metadata: Metadata) -> Result<Self> {
        if !metadata.is_file() {
            return Err(Error::NotFound);
        }
        Ok(Self {
            size: metadata.len(),
            modified: metadata.modified().map_err(|_| changed())?,
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
            #[cfg(windows)]
            created: {
                use std::os::windows::fs::MetadataExt;
                metadata.creation_time()
            },
        })
    }
}

impl RetainedFile {
    pub fn new(audio: AudioFile) -> Result<Self> {
        let stamp = Stamp::read(audio.file.metadata().map_err(|_| changed())?)?;
        if stamp.size != audio.length {
            return Err(changed());
        }
        Ok(Self {
            file: audio.file,
            stamp,
            length: audio.length,
            content_type: audio.content_type,
        })
    }

    pub fn check(&self) -> Result<()> {
        let current = Stamp::read(self.file.metadata().map_err(|_| changed())?)?;
        if current != self.stamp {
            return Err(changed());
        }
        Ok(())
    }

    pub async fn read(
        self: &Arc<Self>,
        entry: Arc<Entry>,
        control: Arc<BodyControl>,
        offset: u64,
        count: usize,
    ) -> Result<Bytes> {
        let file = self.clone();
        // The lease owner awaits this worker even after its caller or lease is retired.
        tokio::task::spawn_blocking(move || {
            entry.alive()?;
            control.check()?;
            if let Err(error) = file.check() {
                entry.stop();
                return Err(error);
            }
            let mut buffer = vec![0; count.min(65536)];
            #[cfg(unix)]
            let read = {
                use std::os::unix::fs::FileExt;
                file.file.read_at(&mut buffer, offset)
            };
            #[cfg(windows)]
            let read = {
                use std::os::windows::fs::FileExt;
                file.file.seek_read(&mut buffer, offset)
            };
            #[cfg(not(any(unix, windows)))]
            let read: std::io::Result<usize> =
                Err(std::io::Error::other("Unsupported file platform"));
            #[cfg(test)]
            if let Ok(count) = &read {
                entry
                    .bytes_read
                    .fetch_add(*count as u64, std::sync::atomic::Ordering::Relaxed);
            }
            #[cfg(test)]
            if let Some(pause) = entry.read_pause.get() {
                pause.pause()?;
            }
            if let Err(error) = file.check() {
                entry.stop();
                return Err(error);
            }
            entry.alive()?;
            control.check()?;
            let read = read.map_err(|_| Error::Upstream("Audio file could not be read"))?;
            buffer.truncate(read);
            Ok(Bytes::from(buffer))
        })
        .await
        .map_err(|_| Error::Internal("Playback file worker stopped".into()))?
    }
}

fn changed() -> Error {
    Error::Conflict("Audio file changed during playback".into())
}
