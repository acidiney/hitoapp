use hito_audio::test_support as support;
use std::{
    io::{Read, Seek, SeekFrom},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

struct OwnedInput {
    bytes: std::io::Cursor<Vec<u8>>,
    released: Arc<AtomicBool>,
}
impl Read for OwnedInput {
    fn read(&mut self, bytes: &mut [u8]) -> std::io::Result<usize> {
        self.bytes.read(bytes)
    }
}
impl Seek for OwnedInput {
    fn seek(&mut self, position: SeekFrom) -> std::io::Result<u64> {
        self.bytes.seek(position)
    }
}
impl hito_audio::Input for OwnedInput {
    fn is_seekable(&self) -> bool {
        true
    }
    fn byte_len(&self) -> Option<u64> {
        Some(self.bytes.get_ref().len() as u64)
    }
}
impl Drop for OwnedInput {
    fn drop(&mut self) {
        self.released.store(true, Ordering::Release);
    }
}

#[test]
fn an_owned_stream_is_decoded_and_released_with_its_decoder() {
    let mut file = support::wave(48_000, 2, 4800, |_, _| 1234);
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).unwrap();
    let released = Arc::new(AtomicBool::new(false));
    let mut decoder = hito_audio::Decoder::open(OwnedInput {
        bytes: std::io::Cursor::new(bytes),
        released: released.clone(),
    })
    .unwrap();
    let mut samples = 0;
    while let Some(block) = decoder.read().unwrap() {
        assert!(
            block
                .samples
                .iter()
                .all(|sample| (*sample - 1234. / 32768.).abs() < 0.000001)
        );
        samples += block.samples.len();
    }
    assert_eq!(samples, 9600);
    assert!(!released.load(Ordering::Acquire));
    drop(decoder);
    assert!(released.load(Ordering::Acquire));
}
