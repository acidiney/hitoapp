use std::{
    io::Write,
    sync::{Arc, Condvar, Mutex},
};
use tokio::sync::Notify;

#[derive(Default)]
struct State {
    armed: bool,
    released: bool,
    publication: bool,
    selected: usize,
}
#[derive(Clone, Default)]
pub struct Events {
    state: Arc<(Mutex<State>, Condvar)>,
    changed: Arc<Notify>,
}
pub struct Release(Events);
impl Drop for Release {
    fn drop(&mut self) {
        self.0.release();
    }
}
impl Events {
    pub fn arm(&self) -> Release {
        self.state.0.lock().unwrap().armed = true;
        Release(self.clone())
    }
    pub fn release(&self) {
        self.state.0.lock().unwrap().released = true;
        self.state.1.notify_all();
    }
    pub async fn publication(&self) {
        self.wait(|state| state.publication).await;
    }
    pub async fn selected(&self, count: usize) {
        self.wait(|state| state.selected >= count).await;
    }
    async fn wait(&self, ready: impl Fn(&State) -> bool) {
        loop {
            let changed = self.changed.notified();
            if ready(&self.state.0.lock().unwrap()) {
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
        let mut state = self.state.0.lock().unwrap();
        match event["fields"]["event"].as_str() {
            Some("provider_refresh_write_started") => state.publication = true,
            Some("player_source_selected") => state.selected += 1,
            Some("native_session_finished") if state.armed => {
                state.armed = false;
                self.changed.notify_one();
                drop(state);
                tokio::task::block_in_place(|| {
                    let mut state = self.state.0.lock().unwrap();
                    while !state.released {
                        state = self.state.1.wait(state).unwrap();
                    }
                });
                return Ok(bytes.len());
            }
            _ => (),
        }
        self.changed.notify_one();
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
