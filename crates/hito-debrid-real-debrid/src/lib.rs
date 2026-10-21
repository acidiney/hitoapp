mod client;
mod error;
mod torrent;
mod transport;
mod unrestriction;

use hito_remote_audio::network;
pub use hito_remote_audio::{AudioFormat, AudioKind, Pcm16WavFormat};

pub type MediaAllowlist = hito_remote_audio::MediaAllowlist<Error>;
pub type OpenedBoundAudio = hito_remote_audio::OpenedBoundAudio<TorrentFileBinding, Error>;
pub use client::RealDebridClient;
pub use error::Error;
pub use torrent::{TorrentFileBinding, TorrentObservation, TorrentPhase};
pub use unrestriction::UnrestrictionReceipt;

#[cfg(feature = "test-support")]
#[doc(hidden)]
pub mod test_support;

#[cfg(test)]
mod tests;
