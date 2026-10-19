mod binding;
mod client;
mod error;
mod magnet;
mod prepare;
mod submit;
mod transport;

pub use binding::MagnetFileBinding;
pub use client::AllDebridClient;
pub use error::Error;
pub use magnet::{MagnetObservation, MagnetPhase};
pub use prepare::DelayedFileReceipt;
pub use submit::{MagnetFileSelection, MagnetUploadReceipt};
