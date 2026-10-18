use hito_audio::Decoder;
use std::io::Cursor;

pub const SAMPLE_RATE: u32 = 48_000;
pub const CHANNELS: u16 = 2;
pub const FRAMES: u64 = 96_000;
pub const WAV_BYTES: usize = 384_044;

fn sample(index: usize) -> i16 {
    let value = (((index / usize::from(CHANNELS)) % 257) as i16 - 128) * 128;
    if index.is_multiple_of(usize::from(CHANNELS)) {
        value
    } else {
        -value
    }
}

pub fn wav() -> Vec<u8> {
    let data_bytes = (FRAMES * u64::from(CHANNELS) * 2) as u32;
    let mut wav = Vec::with_capacity(WAV_BYTES);
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(data_bytes + 36).to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    wav.extend_from_slice(&16_u32.to_le_bytes());
    wav.extend_from_slice(&1_u16.to_le_bytes());
    wav.extend_from_slice(&CHANNELS.to_le_bytes());
    wav.extend_from_slice(&SAMPLE_RATE.to_le_bytes());
    wav.extend_from_slice(&(SAMPLE_RATE * u32::from(CHANNELS) * 2).to_le_bytes());
    wav.extend_from_slice(&(CHANNELS * 2).to_le_bytes());
    wav.extend_from_slice(&16_u16.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&data_bytes.to_le_bytes());
    for index in 0..(FRAMES * u64::from(CHANNELS)) as usize {
        wav.extend_from_slice(&sample(index).to_le_bytes());
    }
    wav
}

pub fn assert_decoded(bytes: Vec<u8>) {
    let mut decoder = Decoder::open(Cursor::new(bytes)).expect("owned WAV must decode");
    assert_eq!(decoder.format().sample_rate, SAMPLE_RATE);
    assert_eq!(decoder.format().channels, CHANNELS);
    let mut samples = 0;
    while let Some(block) = decoder.read().expect("owned WAV must decode through EOF") {
        for actual in block.samples {
            let expected = f32::from(sample(samples)) / 32768.0;
            assert!((*actual - expected).abs() < 0.000_001);
            samples += 1;
        }
    }
    assert_eq!(samples as u64, FRAMES * u64::from(CHANNELS));
}
