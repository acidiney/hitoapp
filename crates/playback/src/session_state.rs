use std::time::Duration;

#[derive(Debug, Clone, Copy)]
pub enum Command {
    Resume,
    Pause,
    Seek(Duration),
    Volume(f32),
    Speed(f32),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum Error {
    #[error("Playback authorization was withdrawn")]
    Access,
    #[error("The audio input could not be opened")]
    Input,
    #[error("{0}")]
    Native(hito_audio::Error),
    #[error("Playback has stopped")]
    Stopped,
    #[error("Playback controls are busy")]
    Busy,
    #[error("The playback owner failed")]
    Worker,
}
pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Opening,
    Starting,
    Ready,
    Stopped,
    Revoked,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Snapshot {
    pub status: Status,
    pub playing: bool,
    pub buffer_ready: bool,
    pub ended: bool,
    pub position: Duration,
    pub speed: u16,
    pub track: u64,
    pub format: Option<hito_audio::AudioFormat>,
    pub error: Option<Error>,
}
impl Snapshot {
    pub(crate) fn starting() -> Self {
        Self {
            status: Status::Opening,
            playing: false,
            buffer_ready: false,
            ended: false,
            position: Duration::ZERO,
            speed: 1000,
            track: 0,
            format: None,
            error: None,
        }
    }
    pub(crate) fn from_audio(audio: hito_audio::Snapshot) -> Self {
        Self {
            status: Status::Ready,
            playing: audio.playing,
            buffer_ready: audio.buffer_ready,
            ended: audio.ended,
            position: audio.position,
            speed: (audio.speed * 1000.).round() as u16,
            track: audio.track,
            format: Some(audio.format),
            error: audio.error.map(Error::Native),
        }
    }
}
