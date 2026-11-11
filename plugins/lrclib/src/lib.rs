mod catalog;
mod client;
mod config;
mod cooldown;
mod error;
mod http;
mod lrc;
mod lyrics;
mod recording;
mod shutdown;

pub use config::{Config, InitError};
pub use http::router;
pub use shutdown::Shutdown;
