mod client;
mod cover_art;
mod discography;
mod edition;
mod edition_listing;
mod error;
mod identifiers;
mod projection;
mod search;
mod transport;

pub use client::MusicBrainzClient;
pub use cover_art::{CoverArt, CoverArtClient, CoverKind};
pub use error::Error;
pub use identifiers::{
    ArtistRecording, ArtistRecordings, GenreEntity, IsrcRecording, MAX_PROBES, RecordingProbe,
};
pub use search::{NAME_SEARCH_LIMIT, NameCandidate, NameSearch};

#[cfg(test)]
mod tests;
