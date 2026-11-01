use crate::AudioFormat;

pub const MIN_SPEED: f32 = 0.5;
pub const MAX_SPEED: f32 = 3.;
const MIN_RATE: u32 = 8_000;
const MAX_OUTPUT_RATE: u32 = 192_000;
const DECIMATED_RATES: [u32; 4] = [352_800, 384_000, 705_600, 768_000];
const MAX_CHANNELS: u16 = 32;

pub(crate) fn supported(sample_rate: u32, channels: u16) -> bool {
    let rate = (MIN_RATE..=MAX_OUTPUT_RATE).contains(&sample_rate)
        || DECIMATED_RATES.contains(&sample_rate);
    rate && (1..=MAX_CHANNELS).contains(&channels)
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SpeedCapability {
    UnitOnly,
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
