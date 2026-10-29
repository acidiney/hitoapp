use crate::AudioFormat;

pub const MIN_SPEED: f32 = 0.5;
pub const MAX_SPEED: f32 = 3.;
pub(crate) const MAX_OUTPUT_RATE: u32 = 192_000;
const MIN_RATE: u32 = 8_000;
const MAX_RATE: u32 = 384_000;
const MAX_CHANNELS: u16 = 32;

/// The playback speeds the engine can present for one decoded source.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SpeedCapability {
    /// Only unit speed can be presented.
    UnitOnly,
    /// Every finite rate from `min` to `max`, including unit speed.
    Range { min: f32, max: f32 },
}

impl SpeedCapability {
    pub fn supports(&self, rate: f32) -> bool {
        match *self {
            Self::UnitOnly => rate == 1.,
            Self::Range { min, max } => rate.is_finite() && (min..=max).contains(&rate),
        }
    }
}

impl AudioFormat {
    /// Speed support derived from the decoded representation. The engine converts
    /// every representation the decoder accepts before time-stretching it, so a
    /// source outside the decoder's bounds is the only unit-only case.
    pub fn speed_capability(&self) -> SpeedCapability {
        if (MIN_RATE..=MAX_RATE).contains(&self.sample_rate)
            && (1..=MAX_CHANNELS).contains(&self.channels)
        {
            SpeedCapability::Range {
                min: MIN_SPEED,
                max: MAX_SPEED,
            }
        } else {
            SpeedCapability::UnitOnly
        }
    }

    /// The representation sent to the output device: the source itself unless it
    /// is above 192 kHz or has more than two channels.
    pub fn output_format(&self) -> AudioFormat {
        AudioFormat {
            sample_rate: self.sample_rate / self.decimation(),
            channels: self.channels.min(2),
            duration: self.duration,
        }
    }

    pub fn requires_conversion(&self) -> bool {
        self.decimation() > 1 || self.channels > 2
    }

    pub(crate) fn decimation(&self) -> u32 {
        self.sample_rate.div_ceil(MAX_OUTPUT_RATE).max(1)
    }
}
