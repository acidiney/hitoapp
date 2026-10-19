mod binding;
mod client;
mod delayed;
mod error;
mod magnet;
mod media;
mod prepare;
mod submit;
mod transport;

pub use binding::MagnetFileBinding;
pub use client::AllDebridClient;
pub use delayed::{DelayedFileReceipt, DelayedPoll, GeneratedFileCandidate};
pub use error::Error;
pub use magnet::{MagnetObservation, MagnetPhase};
pub use submit::{MagnetFileSelection, MagnetUploadReceipt};

pub use hito_remote_audio::Pcm16WavFormat;
pub type MediaAllowlist = hito_remote_audio::MediaAllowlist<Error>;
pub type OpenedBoundAudio = hito_remote_audio::OpenedBoundAudio<MagnetFileBinding, Error>;
