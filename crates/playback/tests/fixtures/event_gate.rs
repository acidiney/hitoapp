use std::{
    io::Write,
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};
use tokio::sync::Notify;

#[derive(Clone)]
pub struct EventGate {
    event: &'static str,
    entered: Arc<Notify>,
    released: Arc<(Mutex<bool>, Condvar)>,
    armed: Arc<AtomicBool>,
}
pub struct Release(EventGate);
impl Drop for Release {
    fn drop(&mut self) {
        self.0.release();
    }
}
impl EventGate {
    pub fn new(event: &'static str) -> Self {
        Self {
            event,
            entered: Arc::default(),
            released: Arc::default(),
            armed: Arc::default(),
        }
    }
    pub fn arm(&self) {
        self.armed.store(true, Ordering::Release);
    }
    pub fn guard(&self) -> Release {
        Release(self.clone())
    }
    pub async fn entered(&self) {
        self.entered.notified().await;
    }
    pub fn release(&self) {
        *self.released.0.lock().unwrap() = true;
        self.released.1.notify_all();
    }
}
impl Write for EventGate {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        let Ok(event) = serde_json::from_slice::<serde_json::Value>(bytes) else {
            return Ok(bytes.len());
        };
        if event["fields"]["event"] == self.event && self.armed.swap(false, Ordering::AcqRel) {
            self.entered.notify_one();
            tokio::task::block_in_place(|| {
                let mut released = self.released.0.lock().unwrap();
                while !*released {
                    released = self.released.1.wait(released).unwrap();
                }
            });
        }
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
