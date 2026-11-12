pub(crate) mod artwork;
pub(crate) mod publisher;
mod threaded;

pub use artwork::MediaArtwork;
use std::sync::Arc;
use tokio::sync::mpsc;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MediaTrack {
    pub id: uuid::Uuid,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub duration_ms: Option<u64>,
    pub artwork: Option<Arc<MediaArtwork>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaPlayback {
    Playing { position_ms: u64, rate: u16 },
    Paused { position_ms: u64 },
}

pub trait MediaSink: Send + 'static {
    fn set_track(&mut self, track: &MediaTrack);
    fn set_playback(&mut self, playback: MediaPlayback);
    fn clear(&mut self);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaCommand {
    Play,
    Pause,
    Toggle,
    Next,
    Previous,
    Stop,
    Seek { position_ms: u64 },
    SeekTrack { track: uuid::Uuid, position_us: u64 },
    SeekBy { forward: bool, delta_ms: u64 },
}

pub type MediaCommandSender = mpsc::Sender<MediaCommand>;

pub struct MediaIntegration {
    pub(crate) sink: Box<dyn MediaSink>,
    pub(crate) commands: mpsc::Receiver<MediaCommand>,
}

impl MediaIntegration {
    pub fn new(sink: impl MediaSink, commands: mpsc::Receiver<MediaCommand>) -> Self {
        Self {
            sink: Box::new(threaded::ThreadedSink::spawn(sink)),
            commands,
        }
    }
}

pub fn media_commands() -> (MediaCommandSender, mpsc::Receiver<MediaCommand>) {
    mpsc::channel(16)
}
