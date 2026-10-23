use serde_json::Value;
use std::{
    io::Write,
    sync::{Arc, Mutex},
};

#[derive(Default)]
struct Captured {
    bytes: Vec<u8>,
    overflow: bool,
}

#[derive(Clone, Default)]
pub(super) struct Logs(Arc<Mutex<Captured>>);

impl Logs {
    pub fn subscriber(&self) -> impl tracing::Subscriber + Send + Sync + 'static {
        hito_core::test_support::tracing_anchor::ensure();
        let writer = self.clone();
        tracing_subscriber::fmt()
            .with_env_filter("off,hito_core=trace,hito_server=trace")
            .json()
            .without_time()
            .with_ansi(false)
            .with_writer(move || writer.clone())
            .finish()
    }

    pub fn assert_private(&self, canaries: &[String]) -> Vec<Value> {
        let captured = self.0.lock().unwrap();
        assert!(
            !captured.overflow,
            "The owned HTTP diagnostic capture must remain bounded"
        );
        let text = std::str::from_utf8(&captured.bytes).unwrap();
        for canary in canaries {
            assert!(!canary.is_empty());
            let encoded = serde_json::to_string(canary).unwrap();
            assert!(
                !text.contains(canary) && !text.contains(&encoded[1..encoded.len() - 1]),
                "HTTP identity logs must exclude every private canary"
            );
        }
        text.lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect()
    }
}

impl Write for Logs {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        let mut captured = self.0.lock().unwrap();
        if bytes.len() <= 262_144_usize.saturating_sub(captured.bytes.len()) {
            captured.bytes.extend_from_slice(bytes);
        } else {
            captured.overflow = true;
        }
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
