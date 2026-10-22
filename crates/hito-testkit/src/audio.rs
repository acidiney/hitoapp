pub const SAMPLE_RATE: u32 = 48_000;
pub const CHANNELS: u16 = 2;
pub const FRAMES: u64 = 96_000;
pub const WAV_BYTES: usize = 384_044;

pub fn ramp_sample(index: usize) -> i16 {
    let value = (((index / usize::from(CHANNELS)) % 257) as i16 - 128) * 128;
    if index.is_multiple_of(usize::from(CHANNELS)) {
        value
    } else {
        -value
    }
}

pub fn pcm16_wav(
    sample_rate: u32,
    channels: u16,
    frames: u64,
    sample: impl Fn(usize) -> i16,
) -> Vec<u8> {
    let data_bytes = (frames * u64::from(channels) * 2) as u32;
    let mut wav = Vec::with_capacity(data_bytes as usize + 44);
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(data_bytes + 36).to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    wav.extend_from_slice(&16_u32.to_le_bytes());
    wav.extend_from_slice(&1_u16.to_le_bytes());
    wav.extend_from_slice(&channels.to_le_bytes());
    wav.extend_from_slice(&sample_rate.to_le_bytes());
    wav.extend_from_slice(&(sample_rate * u32::from(channels) * 2).to_le_bytes());
    wav.extend_from_slice(&(channels * 2).to_le_bytes());
    wav.extend_from_slice(&16_u16.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&data_bytes.to_le_bytes());
    for index in 0..(frames * u64::from(channels)) as usize {
        wav.extend_from_slice(&sample(index).to_le_bytes());
    }
    wav
}

pub fn ramp_wav() -> Vec<u8> {
    pcm16_wav(SAMPLE_RATE, CHANNELS, FRAMES, ramp_sample)
}

pub struct RampVerifier {
    seen: usize,
    expected_samples: u64,
}

impl RampVerifier {
    pub fn new(expected_samples: u64) -> Self {
        Self {
            seen: 0,
            expected_samples,
        }
    }

    pub fn verify(&mut self, samples: &[f32]) {
        for actual in samples {
            let expected = f32::from(ramp_sample(self.seen)) / 32768.0;
            assert!((*actual - expected).abs() < 0.000_001);
            self.seen += 1;
        }
    }

    pub fn finish(self) {
        assert_eq!(self.seen as u64, self.expected_samples);
    }
}
