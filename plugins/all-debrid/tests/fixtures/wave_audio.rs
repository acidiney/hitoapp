use hito_audio::Decoder;
use std::io::Cursor;

pub use hito_testkit::audio::*;

pub fn assert_decoded(bytes: Vec<u8>) {
    let mut decoder = Decoder::open(Cursor::new(bytes)).expect("owned WAV must decode");
    assert_eq!(decoder.format().sample_rate, SAMPLE_RATE);
    assert_eq!(decoder.format().channels, CHANNELS);
    let mut verifier = RampVerifier::new(FRAMES * u64::from(CHANNELS));
    while let Some(block) = decoder.read().expect("owned WAV must decode through EOF") {
        verifier.verify(block.samples);
    }
    verifier.finish();
}
