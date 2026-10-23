use serde_json::Value;
use std::{
    io::Write,
    sync::{Arc, Mutex},
};
use tokio::sync::Notify;

#[derive(Default)]
struct Captured {
    bytes: Vec<u8>,
    events: Vec<Value>,
}
#[derive(Clone, Default)]
pub struct Events {
    captured: Arc<Mutex<Captured>>,
    changed: Arc<Notify>,
}
impl Events {
    pub fn count(&self, name: &str) -> usize {
        self.captured
            .lock()
            .unwrap()
            .events
            .iter()
            .filter(|event| event["fields"]["event"] == name)
            .count()
    }
    pub async fn after(&self, name: &str, count: usize) -> Value {
        loop {
            let changed = self.changed.notified();
            if let Some(event) = self
                .captured
                .lock()
                .unwrap()
                .events
                .iter()
                .filter(|event| event["fields"]["event"] == name)
                .nth(count)
            {
                return event.clone();
            }
            changed.await;
        }
    }
    pub fn text(&self) -> String {
        String::from_utf8(self.captured.lock().unwrap().bytes.clone()).unwrap()
    }
}
impl Write for Events {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        let mut state = self.captured.lock().unwrap();
        state.bytes.extend_from_slice(bytes);
        if let Ok(event) = serde_json::from_slice(bytes) {
            state.events.push(event);
        }
        drop(state);
        self.changed.notify_one();
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
