mod client;
mod error;
mod torrent;
mod transport;
mod unrestriction;

pub use client::RealDebridClient;
pub use error::Error;
pub use torrent::{TorrentFileBinding, TorrentObservation, TorrentPhase};
pub use unrestriction::UnrestrictionReceipt;

#[cfg(test)]
mod tests;
