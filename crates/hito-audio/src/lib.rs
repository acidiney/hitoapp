mod decoding;
mod error;

pub use decoding::{AudioFormat, Decoded, Decoder};
pub use error::{Error, Result};
pub use symphonia::core::io::MediaSource as Input;

#[cfg(feature = "device")]
mod device;
#[cfg(feature = "device")]
mod player;
#[cfg(feature = "device")]
mod processor;
#[cfg(feature = "device")]
mod presentation;
#[cfg(feature = "device")]
mod render;
#[cfg(feature = "device")]
mod state;
#[cfg(feature = "device")]
mod tempo;
#[cfg(feature = "device")]
mod worker;

#[cfg(feature = "device")]
pub use player::{Player, Stop};
#[cfg(feature = "device")]
pub use state::Snapshot;
#[cfg(feature = "native-probe")]
#[doc(hidden)]
pub use processor::native_processor_counts;
