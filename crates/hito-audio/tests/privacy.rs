use hito_audio::test_support as support;

use std::{
    io::{Read, Seek, SeekFrom, Write},
    sync::{Arc, Mutex},
};

#[derive(Clone)]
struct Logs(Arc<Mutex<Vec<u8>>>);

impl Write for Logs {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[test]
fn vendor_diagnostics_do_not_log_private_media_bytes_at_trace_level() {
    let logs = Logs(Arc::new(Mutex::new(Vec::new())));
    let captured = logs.clone();
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::TRACE)
        .with_ansi(false)
        .without_time()
        .with_writer(move || captured.clone())
        .try_init()
        .unwrap();
    let mut input = support::wave(48_000, 1, 96, |_, _| 0);
    let mut bytes = Vec::new();
    input.read_to_end(&mut bytes).unwrap();
    bytes.splice(12..12, *b"PRIV\0\0\0\0");
    let length = bytes.len() as u32 - 8;
    bytes[4..8].copy_from_slice(&length.to_le_bytes());
    input.seek(SeekFrom::Start(0)).unwrap();
    input.write_all(&bytes).unwrap();
    input.seek(SeekFrom::Start(0)).unwrap();
    let mut decoder = hito_audio::Decoder::open(input).unwrap();
    while decoder.read().unwrap().is_some() {}
    let output = String::from_utf8(logs.0.lock().unwrap().clone()).unwrap();
    assert!(output.contains("audio_decoder_ready"));
    assert!(
        !output.contains("PRIV"),
        "vendor input bytes escaped into logs"
    );
}
