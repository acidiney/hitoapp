mod pcm;
mod symphonia;

use crate::{Input, Result};
use ::symphonia::core::audio::Channels;
pub use pcm::PcmSource;
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AudioFormat {
    pub sample_rate: u32,
    pub channels: u16,
    pub duration: Option<Duration>,
}

pub struct Decoded<'a> {
    pub samples: &'a [f32],
    pub position: Duration,
}

pub struct Decoder {
    backend: Backend,
}

enum Backend {
    Symphonia(symphonia::Decoder),
    Pcm(pcm::Decoder),
}

impl Decoder {
    pub fn open(input: impl Input + 'static) -> Result<Self> {
        symphonia::Decoder::open(input).map(|decoder| Self {
            backend: Backend::Symphonia(decoder),
        })
    }

    pub fn from_pcm(source: impl PcmSource + 'static) -> Result<Self> {
        pcm::Decoder::open(source).map(|decoder| Self {
            backend: Backend::Pcm(decoder),
        })
    }

    pub fn format(&self) -> AudioFormat {
        match &self.backend {
            Backend::Symphonia(decoder) => decoder.format(),
            Backend::Pcm(decoder) => decoder.format(),
        }
    }

    pub(crate) fn layout(&self) -> &Channels {
        match &self.backend {
            Backend::Symphonia(decoder) => decoder.layout(),
            Backend::Pcm(decoder) => decoder.layout(),
        }
    }

    pub fn read(&mut self) -> Result<Option<Decoded<'_>>> {
        self.read_until(|| false)
    }

    pub fn read_until(
        &mut self,
        mut interrupted: impl FnMut() -> bool,
    ) -> Result<Option<Decoded<'_>>> {
        match &mut self.backend {
            Backend::Symphonia(decoder) => decoder.read_until(&mut interrupted),
            Backend::Pcm(decoder) => decoder.read(&mut interrupted),
        }
    }

    pub fn seek(&mut self, position: Duration) -> Result<()> {
        match &mut self.backend {
            Backend::Symphonia(decoder) => decoder.seek(position),
            Backend::Pcm(decoder) => decoder.seek(position),
        }
    }
}
