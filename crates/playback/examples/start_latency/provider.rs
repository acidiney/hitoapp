use super::{Failure, process::ReaderProcess};
use serde_json::{Value, json};
use std::{
    io::{BufRead, BufReader, Read, Write},
    process::{ChildStdin, Command},
    sync::mpsc,
    time::{Duration, Instant},
};

pub(super) struct ProviderProcess {
    process: ReaderProcess,
    input: ChildStdin,
    events: mpsc::Receiver<Value>,
    poisoned: bool,
    pub(super) address: String,
    pub(super) token: String,
}

impl ProviderProcess {
    pub(super) fn start(script: &str, audio: &str, recordings: usize) -> Result<Self, Failure> {
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut command = Command::new("node");
        command
            .args([script, "--audio", audio, "--recordings"])
            .arg(recordings.to_string());
        Self::start_command(command, deadline)
    }

    fn start_command(command: Command, deadline: Instant) -> Result<Self, Failure> {
        let (sender, events) = mpsc::sync_channel(16);
        let mut process = ReaderProcess::spawn(command, move |stdout| read_events(stdout, sender))?;
        let input = process.child.stdin.take().ok_or("provider_stdin_missing")?;
        let mut owner = Self {
            process,
            input,
            events,
            poisoned: false,
            address: String::new(),
            token: String::new(),
        };
        let listening = owner.next(deadline)?;
        if listening["event"] != "listening" {
            return Err("provider_startup_invalid".into());
        }
        owner.address = listening["address"]
            .as_str()
            .filter(|value| value.len() <= 512)
            .ok_or("provider_address_invalid")?
            .to_owned();
        owner.token = listening["token"]
            .as_str()
            .filter(|value| !value.is_empty() && value.len() <= 512)
            .ok_or("provider_token_invalid")?
            .to_owned();
        if Instant::now() >= deadline {
            return Err("provider_deadline_elapsed".into());
        }
        Ok(owner)
    }

    fn next(&mut self, deadline: Instant) -> Result<Value, Failure> {
        self.process.check_live()?;
        let remaining = deadline
            .checked_duration_since(Instant::now())
            .ok_or("provider_deadline_elapsed")?;
        let value = self
            .events
            .recv_timeout(remaining)
            .map_err(|error| match error {
                mpsc::RecvTimeoutError::Timeout => "provider_deadline_elapsed",
                mpsc::RecvTimeoutError::Disconnected => "provider_reader_closed",
            })?;
        self.process.check_live()?;
        if Instant::now() >= deadline {
            return Err("provider_deadline_elapsed".into());
        }
        Ok(value)
    }

    fn ask_until<T>(
        &mut self,
        command: Value,
        reply: &str,
        deadline: Instant,
        validate: impl FnOnce(Value) -> Result<T, Failure>,
    ) -> Result<T, Failure> {
        if self.poisoned {
            return Err("provider_retired".into());
        }
        let result = self
            .exchange(command, reply, deadline)
            .and_then(validate)
            .and_then(|value| {
                if Instant::now() >= deadline {
                    return Err("provider_deadline_elapsed".into());
                }
                Ok(value)
            });
        if result.is_err() {
            self.poisoned = true;
        }
        result
    }

    fn exchange(
        &mut self,
        command: Value,
        reply: &str,
        deadline: Instant,
    ) -> Result<Value, Failure> {
        self.process.check_live()?;
        let mut encoded = serde_json::to_vec(&command).map_err(|_| "provider_command_invalid")?;
        encoded.push(b'\n');
        if encoded.len() > 512 {
            return Err("provider_command_too_large".into());
        }
        loop {
            if Instant::now() >= deadline {
                return Err("provider_deadline_elapsed".into());
            }
            match self.input.write(&encoded) {
                Ok(count) if count == encoded.len() => break,
                Ok(_) => return Err("provider_partial_write".into()),
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(_) => return Err("provider_write_failed".into()),
            }
        }
        let value = self.next(deadline)?;
        if value["event"] != reply || (reply == "ack" && value["cmd"] != command["cmd"]) {
            return Err("provider_reply_mismatch".into());
        }
        Ok(value)
    }

    fn ask<T>(
        &mut self,
        command: Value,
        reply: &str,
        validate: impl FnOnce(Value) -> Result<T, Failure>,
    ) -> Result<T, Failure> {
        self.ask_until(
            command,
            reply,
            Instant::now() + Duration::from_secs(10),
            validate,
        )
    }

    pub(super) fn latency(&mut self, milliseconds: u64) -> Result<(), Failure> {
        self.ask(json!({"cmd": "latency", "ms": milliseconds}), "ack", |_| {
            Ok(())
        })
    }
    pub(super) fn sources(&mut self, recording: &str, ids: &[&str]) -> Result<(), Failure> {
        let items: Vec<Value> = ids
            .iter()
            .map(|id| json!({"id": id, "label": "Bench", "ready": true}))
            .collect();
        self.ask(
            json!({"cmd": "sources", "recording": recording, "items": items}),
            "ack",
            |_| Ok(()),
        )
    }
    pub(super) fn pace(&mut self, source: &str) -> Result<(), Failure> {
        self.ask(json!({"cmd": "pace", "source": source}), "ack", |_| Ok(()))
    }
    pub(super) fn unpace(&mut self, source: &str) -> Result<(), Failure> {
        self.ask(
            json!({"cmd": "unpace", "source": source}),
            "ack",
            |_| Ok(()),
        )
    }
    pub(super) fn streams(&mut self, source: &str) -> Result<u64, Failure> {
        self.ask(
            json!({"cmd": "streams", "source": source}),
            "streams",
            |value| {
                value["count"]
                    .as_u64()
                    .ok_or_else(|| "provider_count_invalid".into())
            },
        )
    }
    pub(super) fn drop_source(&mut self, source: &str) -> Result<(), Failure> {
        self.ask(json!({"cmd": "drop", "source": source}), "dropped", |_| {
            Ok(())
        })
    }
    pub(super) fn counters(&mut self) -> Result<(u64, u64), Failure> {
        self.ask(json!({"cmd": "stats"}), "stats", |stats| {
            Ok((
                stats["counters"]["sources"]
                    .as_u64()
                    .ok_or("provider_count_invalid")?,
                stats["counters"]["audio"]
                    .as_u64()
                    .ok_or("provider_count_invalid")?,
            ))
        })
    }
    pub(super) fn finish(self) -> Result<(), Failure> {
        self.process.finish()?;
        Ok(())
    }
}

fn read_events(output: impl Read, sender: mpsc::SyncSender<Value>) -> Result<(), &'static str> {
    let mut reader = BufReader::new(output);
    loop {
        let mut line = Vec::new();
        let count = reader
            .by_ref()
            .take(8193)
            .read_until(b'\n', &mut line)
            .map_err(|_| "provider_read_failed")?;
        if count == 0 {
            return Ok(());
        }
        if count > 8192 {
            return Err("provider_line_too_large");
        }
        if !line.ends_with(b"\n") {
            return Err("provider_partial_line");
        }
        let value = serde_json::from_slice(&line).map_err(|_| "provider_invalid_json")?;
        sender
            .try_send(value)
            .map_err(|_| "provider_event_queue_full_or_closed")?;
    }
}

#[cfg(test)]
#[path = "provider/tests.rs"]
mod tests;
