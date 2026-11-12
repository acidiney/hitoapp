use std::{
    io::Read,
    process::{Child, Command, Stdio},
    thread::{self, JoinHandle},
};

type Reader = JoinHandle<Result<(), &'static str>>;

pub(super) struct ReaderProcess {
    pub(super) child: Child,
    reader: Option<Reader>,
    terminal: Option<&'static str>,
    stopped: bool,
}

impl ReaderProcess {
    pub(super) fn spawn(
        mut command: Command,
        read: impl FnOnce(Box<dyn Read + Send>) -> Result<(), &'static str> + Send + 'static,
    ) -> Result<Self, &'static str> {
        let child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|_| "child_spawn_failed")?;
        let mut owner = Self {
            child,
            reader: None,
            terminal: None,
            stopped: false,
        };
        let stdout = owner.child.stdout.take().ok_or("child_stdout_missing")?;
        owner.reader = Some(
            thread::Builder::new()
                .name("benchmark-reader".into())
                .spawn(move || read(Box::new(stdout)))
                .map_err(|_| "reader_spawn_failed")?,
        );
        Ok(owner)
    }

    pub(super) fn check_live(&mut self) -> Result<(), &'static str> {
        if let Some(terminal) = self.terminal {
            return Err(terminal);
        }
        if self.reader.as_ref().is_some_and(JoinHandle::is_finished) {
            let terminal = match self.reader.take().unwrap().join() {
                Ok(Ok(())) => "reader_eof",
                Ok(Err(code)) => code,
                Err(_) => "reader_panicked",
            };
            self.terminal = Some(terminal);
            return Err(terminal);
        }
        if self
            .child
            .try_wait()
            .map_err(|_| "child_status_failed")?
            .is_some()
        {
            return Err("child_exited");
        }
        Ok(())
    }

    pub(super) fn finish(mut self) -> Result<(), &'static str> {
        let live = self.check_live();
        let cleanup = self.stop();
        live.and(cleanup)
    }

    fn stop(&mut self) -> Result<(), &'static str> {
        if self.stopped {
            return Ok(());
        }
        let kill = match self.child.try_wait() {
            Ok(Some(_)) => Ok(()),
            Ok(None) => self.child.kill().map_err(|_| "child_kill_failed"),
            Err(_) => self.child.kill().map_err(|_| "child_kill_failed"),
        };
        let wait = self
            .child
            .wait()
            .map(|_| ())
            .map_err(|_| "child_wait_failed");
        let reader = match self.reader.take() {
            Some(reader) => reader
                .join()
                .map_err(|_| "reader_panicked")
                .and_then(|result| result),
            None => Ok(()),
        };
        self.stopped = true;
        kill.and(wait).and(reader)
    }
}

impl Drop for ReaderProcess {
    fn drop(&mut self) {
        if !self.stopped {
            let outcome = if self.stop().is_ok() {
                "fallback"
            } else {
                "cleanup_failed"
            };
            eprintln!("{{\"event\":\"benchmark_process_cleanup\",\"outcome\":\"{outcome}\"}}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
            mpsc,
        },
        time::{Duration, Instant},
    };

    fn command(code: &str) -> Command {
        let mut command = Command::new("python3");
        command.args(["-c", code]);
        command
    }

    fn wait_for_reader(owner: &ReaderProcess) {
        let deadline = Instant::now() + Duration::from_secs(3);
        while !owner.reader.as_ref().unwrap().is_finished() {
            assert!(Instant::now() < deadline, "owned reader did not terminate");
            thread::yield_now();
        }
    }

    #[test]
    fn live_child_with_closed_stdout_is_not_a_live_observer() {
        let mut owner = ReaderProcess::spawn(
            command("import os,sys; os.close(1); sys.stdin.read()"),
            |mut stdout| {
                std::io::copy(&mut stdout, &mut std::io::sink()).map_err(|_| "reader_io")?;
                Ok(())
            },
        )
        .unwrap();
        let pid = owner.child.id();
        wait_for_reader(&owner);
        assert_eq!(owner.check_live(), Err("reader_eof"));
        assert!(owner.finish().is_err());
        assert!(
            !std::path::Path::new(&format!("/proc/{pid}")).exists(),
            "owned child remained after terminal observer"
        );
    }

    #[test]
    fn reader_panic_is_failure_and_child_is_reaped() {
        let mut owner = ReaderProcess::spawn(
            command("import os,sys; os.write(1,b'x'); sys.stdin.read()"),
            |mut stdout| {
                let mut byte = [0];
                stdout.read_exact(&mut byte).unwrap();
                panic!("owned_reader_failure_control");
            },
        )
        .unwrap();
        let pid = owner.child.id();
        wait_for_reader(&owner);
        assert_eq!(owner.check_live(), Err("reader_panicked"));
        assert!(owner.finish().is_err());
        assert!(
            !std::path::Path::new(&format!("/proc/{pid}")).exists(),
            "owned child remained after reader panic"
        );
    }

    #[test]
    fn explicit_finish_reaps_child_and_joins_live_reader() {
        let (ready, received) = mpsc::channel();
        let joined = Arc::new(AtomicBool::new(false));
        let completed = joined.clone();
        let owner = ReaderProcess::spawn(
            command("import os,sys; os.write(1,b'x'); sys.stdin.read()"),
            move |mut stdout| {
                let mut byte = [0];
                stdout.read_exact(&mut byte).map_err(|_| "reader_io")?;
                ready.send(()).map_err(|_| "test_observer_closed")?;
                std::io::copy(&mut stdout, &mut std::io::sink()).map_err(|_| "reader_io")?;
                completed.store(true, Ordering::SeqCst);
                Ok(())
            },
        )
        .unwrap();
        let pid = owner.child.id();
        received.recv_timeout(Duration::from_secs(3)).unwrap();
        owner.finish().unwrap();
        assert!(
            joined.load(Ordering::SeqCst),
            "reader was not joined before finish returned"
        );
        assert!(
            !std::path::Path::new(&format!("/proc/{pid}")).exists(),
            "owned child was not reaped"
        );
    }

    #[test]
    fn unwinding_constructor_scope_reaps_child_and_joins_reader() {
        let pid;
        let joined = Arc::new(AtomicBool::new(false));
        let completed = joined.clone();
        {
            let (ready, received) = mpsc::channel();
            let owner = ReaderProcess::spawn(
                command("import os,sys; os.write(1,b'x'); sys.stdin.read()"),
                move |mut stdout| {
                    let mut byte = [0];
                    stdout.read_exact(&mut byte).map_err(|_| "reader_io")?;
                    ready.send(()).map_err(|_| "test_observer_closed")?;
                    std::io::copy(&mut stdout, &mut std::io::sink()).map_err(|_| "reader_io")?;
                    completed.store(true, Ordering::SeqCst);
                    Ok(())
                },
            )
            .unwrap();
            pid = owner.child.id();
            received.recv_timeout(Duration::from_secs(3)).unwrap();
        }
        assert!(
            joined.load(Ordering::SeqCst),
            "scope exit detached the reader"
        );
        assert!(
            !std::path::Path::new(&format!("/proc/{pid}")).exists(),
            "scope exit leaked child"
        );
    }
}
