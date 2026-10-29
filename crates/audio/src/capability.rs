use crate::AudioFormat;

pub const MIN_SPEED: f32 = 0.5;
pub const MAX_SPEED: f32 = 3.;
const MIN_RATE: u32 = 8_000;
const MAX_OUTPUT_RATE: u32 = 192_000;
/// Rates above the output limit that decimate by an integer factor onto the
/// 44.1 kHz and 48 kHz families (2x and 4x). Any other rate above the limit would
/// land on a rate output devices do not open, so it is not supported.
const DECIMATED_RATES: [u32; 4] = [352_800, 384_000, 705_600, 768_000];
const MAX_CHANNELS: u16 = 32;

/// Whether the engine can present a decoded representation: a rate the output
/// accepts directly or reaches by decimation, and a bounded channel count.
pub(crate) fn supported(sample_rate: u32, channels: u16) -> bool {
    let rate = (MIN_RATE..=MAX_OUTPUT_RATE).contains(&sample_rate)
        || DECIMATED_RATES.contains(&sample_rate);
    rate && (1..=MAX_CHANNELS).contains(&channels)
}

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
    /// representation the engine does not support is the only unit-only case.
    pub fn speed_capability(&self) -> SpeedCapability {
        if supported(self.sample_rate, self.channels) {
            SpeedCapability::Range {
                min: MIN_SPEED,
                max: MAX_SPEED,
            }
        } else {
            SpeedCapability::UnitOnly
        }
    }

    /// The representation the output path accepts before device negotiation: the
    /// source itself unless it is above 192 kHz or has more than two channels. A
    /// device that does not offer it receives the representation the player
    /// negotiated instead, reported by `Player::output_format`.
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
