use hito_playback::{MediaArtwork, MediaPlayback, MediaSink, MediaTrack};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};
use url::Url;

pub trait Controls: Send + 'static {
    fn metadata(&mut self, metadata: &Metadata<'_>);
    fn playback(&mut self, playing: bool, progress: Duration);
    fn reset(&mut self);
}

pub struct Metadata<'a> {
    pub track_id: uuid::Uuid,
    pub title: &'a str,
    pub artist: Option<&'a str>,
    pub album: Option<&'a str>,
    pub duration: Option<Duration>,
    pub cover_url: Option<&'a str>,
}

pub enum Create<C> {
    Ready(C),
    Later,
    Failed,
}

pub struct Lazy<C: Controls> {
    init: Option<Box<dyn FnMut() -> Create<C> + Send>>,
    controls: Option<C>,
}

impl<C: Controls> Lazy<C> {
    pub fn new(init: impl FnMut() -> Create<C> + Send + 'static) -> Self {
        Self {
            init: Some(Box::new(init)),
            controls: None,
        }
    }

    fn get(&mut self) -> Option<&mut C> {
        if let Some(init) = &mut self.init {
            match init() {
                Create::Ready(controls) => {
                    self.controls = Some(controls);
                    self.init = None;
                }
                Create::Later => {}
                Create::Failed => {
                    self.init = None;
                    tracing::warn!(
                        event = "desktop_system_media_unavailable",
                        "Operating-system media controls are unavailable"
                    );
                }
            }
        }
        self.controls.as_mut()
    }
}

impl<C: Controls> Controls for Lazy<C> {
    fn metadata(&mut self, metadata: &Metadata<'_>) {
        if let Some(controls) = self.get() {
            controls.metadata(metadata);
        }
    }
    fn playback(&mut self, playing: bool, progress: Duration) {
        if let Some(controls) = self.get() {
            controls.playback(playing, progress);
        }
    }
    fn reset(&mut self) {
        if let Some(controls) = self.controls.as_mut() {
            controls.reset();
        }
    }
}

pub struct SystemMedia<C: Controls> {
    controls: C,
    artwork: ArtworkFile,
    shown: Option<(Arc<MediaArtwork>, Url)>,
}

impl<C: Controls> SystemMedia<C> {
    pub fn new(controls: C, artwork_directory: PathBuf) -> Self {
        Self {
            controls,
            artwork: ArtworkFile::new(artwork_directory),
            shown: None,
        }
    }
}

impl<C: Controls> MediaSink for SystemMedia<C> {
    fn set_track(&mut self, track: &MediaTrack) {
        let cover = track.artwork.as_ref().and_then(|artwork| {
            if let Some((shown, url)) = &self.shown
                && Arc::ptr_eq(shown, artwork)
            {
                return Some(url.clone());
            }
            let url = self.artwork.write(artwork.content_type, &artwork.bytes)?;
            self.shown = Some((artwork.clone(), url.clone()));
            Some(url)
        });
        if cover.is_none() {
            self.shown = None;
            self.artwork.remove();
        }
        self.controls.metadata(&Metadata {
            track_id: track.id,
            title: &track.title,
            artist: Some(track.artist.as_str()).filter(|artist| !artist.is_empty()),
            album: Some(track.album.as_str()).filter(|album| !album.is_empty()),
            duration: track.duration_ms.map(Duration::from_millis),
            cover_url: cover.as_ref().map(Url::as_str),
        });
    }

    fn set_playback(&mut self, playback: MediaPlayback) {
        match playback {
            MediaPlayback::Playing { position_ms, .. } => self
                .controls
                .playback(true, Duration::from_millis(position_ms)),
            MediaPlayback::Paused { position_ms } => self
                .controls
                .playback(false, Duration::from_millis(position_ms)),
        }
    }

    fn clear(&mut self) {
        self.controls.reset();
        self.shown = None;
        self.artwork.remove();
    }
}

impl<C: Controls> Drop for SystemMedia<C> {
    fn drop(&mut self) {
        self.controls.reset();
        self.artwork.remove();
    }
}

struct ArtworkFile {
    directory: PathBuf,
    current: Option<PathBuf>,
    serial: u64,
}

impl ArtworkFile {
    fn new(directory: PathBuf) -> Self {
        let _ = fs::remove_dir_all(&directory);
        Self {
            directory,
            current: None,
            serial: 0,
        }
    }

    fn write(&mut self, content_type: &str, bytes: &[u8]) -> Option<Url> {
        self.remove();
        let extension = match content_type {
            "image/png" => "png",
            "image/jpeg" => "jpg",
            _ => return None,
        };
        self.serial += 1;
        let path = self
            .directory
            .join(format!("cover-{}.{extension}", self.serial));
        match private_write(&self.directory, &path, bytes) {
            Ok(()) => {
                let url = Url::from_file_path(&path).ok();
                self.current = Some(path);
                url
            }
            Err(_) => {
                tracing::debug!(
                    event = "desktop_system_media_artwork_failed",
                    "Artwork could not be prepared for system media"
                );
                let _ = fs::remove_file(&path);
                None
            }
        }
    }

    fn remove(&mut self) {
        if let Some(path) = self.current.take() {
            let _ = fs::remove_file(path);
        }
    }
}

fn private_write(directory: &Path, path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    let mut builder = fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
    builder.create(directory)?;
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    options.open(path)?.write_all(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    #[derive(Debug, PartialEq)]
    enum Call {
        Metadata {
            title: String,
            artist: Option<String>,
            album: Option<String>,
            duration: Option<Duration>,
            cover: Option<String>,
        },
        Playback(bool, Duration),
        Reset,
    }
    #[derive(Clone, Default)]
    struct Fake(Arc<Mutex<Vec<Call>>>);
    impl Controls for Fake {
        fn metadata(&mut self, metadata: &Metadata<'_>) {
            self.0.lock().unwrap().push(Call::Metadata {
                title: metadata.title.into(),
                artist: metadata.artist.map(Into::into),
                album: metadata.album.map(Into::into),
                duration: metadata.duration,
                cover: metadata.cover_url.map(Into::into),
            });
        }
        fn playback(&mut self, playing: bool, progress: Duration) {
            self.0
                .lock()
                .unwrap()
                .push(Call::Playback(playing, progress));
        }
        fn reset(&mut self) {
            self.0.lock().unwrap().push(Call::Reset);
        }
    }

    fn track(artwork: bool) -> MediaTrack {
        MediaTrack {
            id: uuid::Uuid::new_v4(),
            title: "Song".into(),
            artist: "Artist".into(),
            album: String::new(),
            duration_ms: Some(200_000),
            artwork: artwork.then(|| {
                Arc::new(MediaArtwork {
                    content_type: "image/png",
                    bytes: b"png".to_vec(),
                })
            }),
        }
    }

    #[test]
    fn metadata_state_and_progress_translate_without_empty_fields() {
        let directory = tempfile::tempdir().unwrap();
        let fake = Fake::default();
        let mut media = SystemMedia::new(fake.clone(), directory.path().join("art"));
        media.set_track(&track(false));
        media.set_playback(MediaPlayback::Playing {
            position_ms: 1500,
            rate: 1000,
        });
        media.set_playback(MediaPlayback::Paused { position_ms: 2500 });
        assert_eq!(
            *fake.0.lock().unwrap(),
            vec![
                Call::Metadata {
                    title: "Song".into(),
                    artist: Some("Artist".into()),
                    album: None,
                    duration: Some(Duration::from_secs(200)),
                    cover: None,
                },
                Call::Playback(true, Duration::from_millis(1500)),
                Call::Playback(false, Duration::from_millis(2500)),
            ]
        );
    }

    #[test]
    fn artwork_is_an_owner_only_file_url_replaced_per_track_and_removed_on_clear() {
        let directory = tempfile::tempdir().unwrap();
        let art = directory.path().join("art");
        let fake = Fake::default();
        let mut media = SystemMedia::new(fake.clone(), art.clone());
        media.set_track(&track(true));
        let first = match &fake.0.lock().unwrap()[0] {
            Call::Metadata { cover, .. } => cover.clone().unwrap(),
            _ => unreachable!(),
        };
        let first = Url::parse(&first).unwrap();
        assert_eq!(first.scheme(), "file");
        let first = first.to_file_path().unwrap();
        assert_eq!(fs::read(&first).unwrap(), b"png");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&first).unwrap().permissions().mode() & 0o777,
                0o600
            );
            assert_eq!(
                fs::metadata(&art).unwrap().permissions().mode() & 0o777,
                0o700
            );
        }
        media.set_track(&track(true));
        assert!(!first.exists());
        assert_eq!(fs::read_dir(&art).unwrap().count(), 1);
        media.clear();
        assert_eq!(fs::read_dir(&art).unwrap().count(), 0);
        assert_eq!(fake.0.lock().unwrap().last(), Some(&Call::Reset));
    }

    #[test]
    fn stale_artwork_from_a_previous_run_is_removed_at_startup() {
        let directory = tempfile::tempdir().unwrap();
        let art = directory.path().join("art");
        fs::create_dir(&art).unwrap();
        fs::write(art.join("cover-9.png"), b"old").unwrap();
        let _media = SystemMedia::new(Fake::default(), art.clone());
        assert!(!art.join("cover-9.png").exists());
    }

    #[test]
    fn a_republish_of_the_same_artwork_reuses_the_file() {
        let directory = tempfile::tempdir().unwrap();
        let art = directory.path().join("art");
        let fake = Fake::default();
        let mut media = SystemMedia::new(fake.clone(), art.clone());
        let shared = track(true);
        media.set_track(&shared);
        media.set_track(&shared);
        let covers: Vec<_> = fake
            .0
            .lock()
            .unwrap()
            .iter()
            .filter_map(|call| match call {
                Call::Metadata { cover, .. } => cover.clone(),
                _ => None,
            })
            .collect();
        assert_eq!(covers.len(), 2);
        assert_eq!(covers[0], covers[1]);
        assert_eq!(fs::read_dir(&art).unwrap().count(), 1);
    }

    #[test]
    fn controls_wait_for_a_missing_window_and_failure_is_final() {
        let attempts = Arc::new(Mutex::new(0));
        let counter = attempts.clone();
        let mut lazy = Lazy::<Fake>::new(move || {
            let mut attempts = counter.lock().unwrap();
            *attempts += 1;
            match *attempts {
                1 | 2 => Create::Later,
                3 => Create::Ready(Fake::default()),
                _ => Create::Failed,
            }
        });
        lazy.reset();
        assert_eq!(*attempts.lock().unwrap(), 0);
        lazy.playback(true, Duration::ZERO);
        lazy.playback(true, Duration::ZERO);
        assert_eq!(*attempts.lock().unwrap(), 2);
        lazy.playback(true, Duration::ZERO);
        lazy.playback(true, Duration::ZERO);
        assert_eq!(*attempts.lock().unwrap(), 3);

        let failures = Arc::new(Mutex::new(0));
        let counter = failures.clone();
        let mut broken = Lazy::<Fake>::new(move || {
            *counter.lock().unwrap() += 1;
            Create::Failed
        });
        broken.playback(true, Duration::ZERO);
        broken.playback(true, Duration::ZERO);
        assert_eq!(*failures.lock().unwrap(), 1);
    }
}
