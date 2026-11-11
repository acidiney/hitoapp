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
    #[cfg(test)]
    last_drop: Option<SourceDrop>,
    pub(super) address: String,
    pub(super) token: String,
}

#[derive(Clone, Copy)]
pub(super) struct SourceDrop {
    pub before: u64,
    pub after: u64,
    pub at_ms: f64,
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
            #[cfg(test)]
            last_drop: None,
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
        self.streams_until(source, Instant::now() + Duration::from_secs(10))
    }

    fn streams_until(&mut self, source: &str, deadline: Instant) -> Result<u64, Failure> {
        self.ask_until(
            json!({"cmd": "streams", "source": source}),
            "streams",
            deadline,
            |value| {
                value["count"]
                    .as_u64()
                    .ok_or_else(|| "provider_count_invalid".into())
            },
        )
    }
    pub(super) fn drop_source(&mut self, source: &str) -> Result<(), Failure> {
        self.drop_source_until(source, Instant::now() + Duration::from_secs(10))
            .map(|_| ())
    }
    pub(super) fn drop_source_until(
        &mut self,
        source: &str,
        deadline: Instant,
    ) -> Result<SourceDrop, Failure> {
        #[cfg(test)]
        {
            self.last_drop = None;
        }
        let before = self.streams_until(source, deadline)?;
        let at_ms = self.ask_until(
            json!({"cmd": "drop", "source": source}),
            "dropped",
            deadline,
            |value| {
                value["at_ms"]
                    .as_f64()
                    .filter(|value| value.is_finite() && *value >= 0.)
                    .ok_or_else(|| "provider_drop_time_invalid".into())
            },
        )?;
        let after = self.streams_until(source, deadline)?;
        let observed = SourceDrop {
            before,
            after,
            at_ms,
        };
        #[cfg(test)]
        {
            self.last_drop = Some(observed);
        }
        Ok(observed)
    }
    #[cfg(test)]
    pub(super) fn last_drop(&self) -> Option<SourceDrop> {
        self.last_drop
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
