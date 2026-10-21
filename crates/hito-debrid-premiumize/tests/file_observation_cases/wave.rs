use super::{BYTES, binding};
use hito_audio::Decoder;
use hito_debrid_premiumize::OpenedBoundAudio;
use std::{io::Cursor, time::Duration};

const SAMPLE_RATE: u32 = 48_000;
const CHANNELS: u16 = 2;
const FRAMES: u32 = 96_000;

fn sample(index: usize) -> i16 {
    ((index * 73 % 32_000) as i16) - 16_000
}

pub(super) fn bytes() -> Vec<u8> {
    let data_bytes = FRAMES * u32::from(CHANNELS) * 2;
    let mut wave = Vec::with_capacity(data_bytes as usize + 44);
    wave.extend_from_slice(b"RIFF");
    wave.extend_from_slice(&(36 + data_bytes).to_le_bytes());
    wave.extend_from_slice(b"WAVEfmt ");
    wave.extend_from_slice(&16_u32.to_le_bytes());
    wave.extend_from_slice(&1_u16.to_le_bytes());
    wave.extend_from_slice(&CHANNELS.to_le_bytes());
    wave.extend_from_slice(&SAMPLE_RATE.to_le_bytes());
    wave.extend_from_slice(&(SAMPLE_RATE * u32::from(CHANNELS) * 2).to_le_bytes());
    wave.extend_from_slice(&(CHANNELS * 2).to_le_bytes());
    wave.extend_from_slice(&16_u16.to_le_bytes());
    wave.extend_from_slice(b"data");
    wave.extend_from_slice(&data_bytes.to_le_bytes());
    for index in 0..FRAMES as usize * usize::from(CHANNELS) {
        wave.extend_from_slice(&sample(index).to_le_bytes());
    }
    assert_eq!(wave.len() as u64, BYTES);
    wave
}

pub(super) async fn collect(mut opened: OpenedBoundAudio) {
    assert!(opened.binding == binding());
    assert_eq!(
        serde_json::to_vec(&opened.binding).unwrap(),
        serde_json::to_vec(&binding()).unwrap()
    );
    assert_eq!(opened.length, BYTES);
    let hito_remote_audio::AudioFormat::Pcm16Wav(wav_format) = opened.format else {
        panic!("the unchanged WAV policy must return a WAV descriptor");
    };
    assert_eq!(wav_format.sample_rate, SAMPLE_RATE);
    assert_eq!(wav_format.channels, CHANNELS);
    assert_eq!(wav_format.frames, u64::from(FRAMES));
    let mut received = Vec::new();
    while let Some(chunk) = opened
        .next_chunk()
        .await
        .expect("the bound body must reach valid EOF")
    {
        assert!(!chunk.is_empty() && chunk.len() <= 65_536);
        assert!(received.len() + chunk.len() <= BYTES as usize);
        received.extend_from_slice(&chunk);
    }
    assert!(opened.next_chunk().await.unwrap().is_none());
    assert!(opened.binding == binding());
    assert!(received == bytes());
    let mut decoder =
        Decoder::open(Cursor::new(received)).expect("the exact owned PCM must decode");
    assert_eq!(decoder.format().sample_rate, SAMPLE_RATE);
    assert_eq!(decoder.format().channels, CHANNELS);
    assert_eq!(decoder.format().duration, Some(Duration::from_secs(2)));
    let mut samples = 0_usize;
    while let Some(block) = decoder.read().expect("the exact PCM samples must decode") {
        for actual in block.samples {
            assert_eq!(*actual, f32::from(sample(samples)) / 32_768.0);
            samples += 1;
        }
    }
    assert_eq!(samples, FRAMES as usize * usize::from(CHANNELS));
}
