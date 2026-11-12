mod capability;
mod convert;
mod decoding;
mod error;
mod mp4_gapless;
#[cfg(test)]
mod mp4_gapless_tests;
#[cfg(any(feature = "device", test))]
mod negotiate;
mod resample;

pub use capability::{MAX_SPEED, MIN_SPEED, SpeedCapability};
pub use convert::Converter;
pub use decoding::{AudioFormat, Decoded, Decoder, PcmSource};
pub use error::{Error, Result};
pub use symphonia::core::io::MediaSource as Input;

#[cfg(feature = "test-support")]
pub mod test_support;

#[cfg(feature = "device")]
mod buffer_policy;
#[cfg(feature = "device")]
mod buffering;
#[cfg(feature = "device")]
mod device;
#[cfg(feature = "device")]
mod player;
#[cfg(feature = "device")]
mod presentation;
#[cfg(feature = "device")]
mod processor;
#[cfg(feature = "device")]
mod render;
#[cfg(any(feature = "device", loom))]
#[cfg_attr(loom, allow(dead_code))]
mod state;
#[cfg(feature = "device")]
mod tempo;
#[cfg(feature = "device")]
mod worker;

#[cfg(feature = "device")]
pub use player::{Player, Stop};
#[cfg(feature = "native-probe")]
#[doc(hidden)]
pub use processor::{fail_next_native_processor, native_processor_counts};
#[cfg(feature = "device")]
pub use state::Snapshot;
