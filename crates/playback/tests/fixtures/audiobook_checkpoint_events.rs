use std::{
    io::Write,
    sync::{Arc, Mutex},
};
use tokio::sync::Notify;
use uuid::Uuid;

#[derive(Clone, Debug)]
pub struct Attempt {
    pub progress_id: Uuid,
    pub operations: Vec<(String, Uuid)>,
    pub success: Option<bool>,
}

#[derive(Clone, Default)]
pub struct Events {
    attempts: Arc<Mutex<Vec<Attempt>>>,
    changed: Arc<Notify>,
}

impl Events {
    pub async fn failed(&self, count: usize) -> Vec<Attempt> {
        loop {
            let changed = self.changed.notified();
            let failed = self
                .attempts
                .lock()
                .unwrap()
                .iter()
                .filter(|attempt| attempt.success == Some(false))
                .cloned()
                .collect::<Vec<_>>();
            if failed.len() >= count {
                return failed;
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
        let mut attempts = self.attempts.lock().unwrap();
        match event["fields"]["event"].as_str() {
            Some("player_progress_write_started") => {
                assert!(
                    attempts.len() < 128,
                    "Checkpoint attempts exceeded the fixture budget"
                );
                attempts.push(Attempt {
                    progress_id: event["fields"]["operation_id"]
                        .as_str()
                        .unwrap()
                        .parse()
                        .unwrap(),
                    operations: Vec::new(),
                    success: None,
                });
            }
            Some("player_progress_write_finished") => {
                let attempt = attempts.last_mut().unwrap();
                assert_eq!(
                    event["fields"]["operation_id"],
                    attempt.progress_id.to_string()
                );
                attempt.success = Some(event["fields"]["success"].as_bool().unwrap());
            }
            _ if event["fields"]["message"] == "new" && event["span"]["name"] == "action" => {
                let kind = event["span"]["action_kind"].as_str().unwrap();
                if matches!(kind, "progress_set" | "audiobook_chapter_set") {
                    let operation = event["span"]["operation_id"]
                        .as_str()
                        .unwrap()
                        .parse()
                        .unwrap();
                    let attempt = attempts.last_mut().unwrap();
                    assert!(
                        attempt.operations.len() < 2,
                        "A native checkpoint exceeded two operations"
                    );
                    attempt.operations.push((kind.into(), operation));
                }
            }
            _ => (),
        }
        drop(attempts);
        self.changed.notify_one();
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
