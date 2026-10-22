mod binding;
mod client;
mod error;
mod media;
mod observation;
mod persistence;
mod prepare;
#[cfg(feature = "test-support")]
pub mod test_support;
mod transport;
mod workflow;

pub use binding::TorrentFileBinding;
pub use client::{TorBoxClient, TorrentObservation, TorrentPhase};
pub use error::Error;
pub use media::BoundAudioOpening;
pub use persistence::{
    CandidateAuthority, CandidateContext, CandidateKey, PersistenceError, SealedCandidate,
};
pub use prepare::{GeneratedFileCandidate, VerifiedFilePreparation};

pub use hito_remote_audio::Pcm16WavFormat;
pub type MediaAllowlist = hito_remote_audio::MediaAllowlist<Error>;
pub type OpenedBoundAudio = hito_remote_audio::OpenedBoundAudio<TorrentFileBinding, Error>;
