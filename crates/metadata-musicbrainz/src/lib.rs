mod client;
mod discography;
mod edition;
mod edition_listing;
mod error;
mod identifiers;
mod projection;
mod search;
mod transport;

pub use client::MusicBrainzClient;
pub use error::Error;
pub use identifiers::{ArtistRecording, ArtistRecordings, IsrcRecording};

#[cfg(test)]
mod tests;
