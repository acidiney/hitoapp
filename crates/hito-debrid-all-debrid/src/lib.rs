mod binding;
mod client;
mod delayed;
mod error;
mod magnet;
mod prepare;
mod submit;
mod transport;

pub use binding::MagnetFileBinding;
pub use client::AllDebridClient;
pub use delayed::{DelayedFileReceipt, DelayedPoll, GeneratedFileCandidate};
pub use error::Error;
pub use magnet::{MagnetObservation, MagnetPhase};
pub use submit::{MagnetFileSelection, MagnetUploadReceipt};
