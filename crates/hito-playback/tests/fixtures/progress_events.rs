#![allow(dead_code)]
use std::{
    io::Write,
    sync::{Arc, Mutex},
};
use tokio::sync::Notify;
use uuid::Uuid;

#[derive(Default)]
struct State {
    started: Vec<Uuid>,
    joined: bool,
}
#[derive(Clone, Default)]
pub struct Events {
    state: Arc<Mutex<State>>,
    changed: Arc<Notify>,
}
impl Events {
    pub fn started(&self) -> Vec<Uuid> {
        self.state.lock().unwrap().started.clone()
    }
    pub async fn started_after(&self, count: usize) -> Uuid {
        loop {
            let changed = self.changed.notified();
            if let Some(id) = self.state.lock().unwrap().started.get(count) {
                return *id;
            }
            changed.await;
        }
    }
    pub async fn joined(&self) {
        loop {
            let changed = self.changed.notified();
            if self.state.lock().unwrap().joined {
                return;
            }
            changed.await;
        }
    }
}
impl Write for Events {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        let Ok(event) = serde_json::from_slice::<serde_json::Value>(bytes) else {
            return Ok(bytes.len());
        };
        let mut state = self.state.lock().unwrap();
        if event["fields"]["event"] == "player_progress_write_started" {
            state
                .started
                .push(Uuid::parse_str(event["fields"]["operation_id"].as_str().unwrap()).unwrap());
        }
        if event["fields"]["event"] == "player_owner_closed" {
            state.joined = true;
        }
        drop(state);
        self.changed.notify_one();
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
