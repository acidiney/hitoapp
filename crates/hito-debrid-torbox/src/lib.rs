mod binding;
mod client;
mod error;
mod media;
mod observation;
mod prepare;
mod transport;
mod workflow;

pub use binding::TorrentFileBinding;
pub use client::{TorBoxClient, TorrentObservation, TorrentPhase};
pub use error::Error;
pub use prepare::GeneratedFileCandidate;

pub use hito_remote_audio::Pcm16WavFormat;
pub type MediaAllowlist = hito_remote_audio::MediaAllowlist<Error>;
pub type OpenedBoundAudio = hito_remote_audio::OpenedBoundAudio<TorrentFileBinding, Error>;
