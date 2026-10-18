mod client;
mod discography;
mod error;
mod projection;
mod search;
mod transport;

pub use client::MusicBrainzClient;
pub use error::Error;

#[cfg(test)]
mod tests;
