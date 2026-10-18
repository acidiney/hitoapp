mod client;
mod error;
mod torrent;
mod transport;

pub use client::RealDebridClient;
pub use error::Error;
pub use torrent::{TorrentFileBinding, TorrentObservation, TorrentPhase};

#[cfg(test)]
mod tests;
