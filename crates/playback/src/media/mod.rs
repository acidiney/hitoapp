//! Operating-system media integration seam.
//!
//! The playback owner publishes authorized track metadata and coarse playback
//! state to a [`MediaSink`] and receives transport requests through a
//! [`MediaCommand`] channel. Backends contain no playback logic.

pub(crate) mod artwork;
pub(crate) mod publisher;

pub use artwork::MediaArtwork;
use std::sync::Arc;
use tokio::sync::mpsc;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MediaTrack {
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

/// Receives structural changes only. Position is an anchor: backends let the
/// operating system extrapolate while `Playing` and are updated again on
/// pause, resume, seek, speed change and track change.
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
            sink: Box::new(sink),
            commands,
        }
    }
}

/// Creates the bounded command channel a backend feeds from operating-system
/// callbacks. Backends use `try_send` and drop commands when it is full.
pub fn media_commands() -> (MediaCommandSender, mpsc::Receiver<MediaCommand>) {
    mpsc::channel(16)
}
