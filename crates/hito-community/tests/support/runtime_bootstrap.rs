use serde_json::Value;
use std::{
    fs::{File, OpenOptions},
    io::{self, Read, Write},
    net::{SocketAddr, TcpStream},
    os::unix::fs::OpenOptionsExt,
    path::{Path, PathBuf},
    process::{Child, Command, ExitStatus, Stdio},
    time::{Duration, Instant},
};

pub struct Process {
    child: Child,
    exit: Option<ExitStatus>,
    address: SocketAddr,
    log: PathBuf,
}

impl Process {
    pub fn start(
        directory: &Path,
        state: &Path,
        credential: &Path,
        address: SocketAddr,
        filter: &str,
        run: &str,
    ) -> Self {
        let log = directory.join(format!("private-bootstrap-{run}.jsonl"));
        let output = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&log)
            .expect("Bootstrap log setup failed");
        let child = Command::new(env!("CARGO_BIN_EXE_hito-community"))
            .arg("--state-directory")
            .arg(state)
            .arg("--admin-token-file")
            .arg(credential)
            .arg("--listen")
            .arg(address.to_string())
            .env_clear()
            .env("RUST_LOG", filter)
            .stdout(Stdio::from(output.try_clone().unwrap()))
            .stderr(Stdio::from(output))
            .spawn()
            .expect("Community bootstrap spawn failed");
        Self {
            child,
            exit: None,
            address,
            log,
        }
    }

    pub fn ready(&mut self) {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            assert!(
                Instant::now() < deadline,
                "Bootstrap readiness exceeded its deadline"
            );
            let request_deadline = deadline.min(Instant::now() + Duration::from_secs(1));
            if let Ok((status, body)) =
                self.request_until("GET", "/v1/openapi.json", None, "", request_deadline)
            {
                assert_eq!(status, 200);
                let document: Value =
                    serde_json::from_slice(&body).expect("Bootstrap readiness JSON failed");
                assert!(document["openapi"].as_str().unwrap().starts_with("3."));
                assert!(
                    Instant::now() < deadline,
                    "Bootstrap readiness exceeded its deadline"
                );
                return;
            }
            assert!(
                self.status().is_none(),
                "Community exited before bootstrap readiness"
            );
            assert!(
                Instant::now() < deadline,
                "Bootstrap readiness exceeded its deadline"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    pub fn request(
        &self,
        method: &str,
        path: &str,
        token: Option<&str>,
        body: &str,
    ) -> io::Result<(u16, Vec<u8>)> {
        self.request_until(
            method,
            path,
            token,
            body,
            Instant::now() + Duration::from_secs(1),
        )
    }

    fn request_until(
        &self,
        method: &str,
        path: &str,
        token: Option<&str>,
        body: &str,
        deadline: Instant,
    ) -> io::Result<(u16, Vec<u8>)> {
        let mut request = format!(
            "{method} {path}?private-bootstrap-query=private-bootstrap-value HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\nContent-Type: application/json\r\nCookie: private-bootstrap-cookie\r\nX-Private-Request: private-bootstrap-header\r\n"
        );
        if let Some(token) = token {
            request.push_str("Authorization: Bearer ");
            request.push_str(token);
            request.push_str("\r\n");
        }
        request.push_str(&format!("Content-Length: {}\r\n\r\n{body}", body.len()));
        let mut stream = TcpStream::connect_timeout(
            &self.address,
            remaining(deadline)?.min(Duration::from_millis(100)),
        )
        .map_err(|error| io::Error::new(error.kind(), "Bootstrap connection failed"))?;
        let mut pending = request.as_bytes();
        while !pending.is_empty() {
            stream
                .set_write_timeout(Some(remaining(deadline)?))
                .map_err(|_| io::Error::other("Bootstrap write timeout setup failed"))?;
            match stream.write(pending) {
                Ok(0) => {
                    return Err(io::Error::new(
                        io::ErrorKind::WriteZero,
                        "Bootstrap request write stopped",
                    ));
                }
                Ok(written) => pending = &pending[written..],
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(error) => {
                    return Err(io::Error::new(
                        error.kind(),
                        "Bootstrap request write failed",
                    ));
                }
            }
            remaining(deadline)?;
        }
        let mut bytes = Vec::new();
        let mut chunk = [0; 4096];
        loop {
            stream
                .set_read_timeout(Some(remaining(deadline)?))
                .map_err(|_| io::Error::other("Bootstrap read timeout setup failed"))?;
            let count = match stream.read(&mut chunk) {
                Ok(count) => count,
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(error) => {
                    return Err(io::Error::new(
                        error.kind(),
                        "Bootstrap response read failed",
                    ));
                }
            };
            remaining(deadline)?;
            if count == 0 {
                break;
            }
            if bytes.len() + count > 262_144 {
                return Err(io::Error::other("Bootstrap response exceeded its bound"));
            }
            bytes.extend_from_slice(&chunk[..count]);
        }
        let header_end = bytes
            .windows(4)
            .position(|part| part == b"\r\n\r\n")
            .ok_or_else(|| io::Error::other("Bootstrap response headers missing"))?;
        let headers = std::str::from_utf8(&bytes[..header_end])
            .map_err(|_| io::Error::other("Bootstrap response headers invalid"))?;
        let status = headers
            .lines()
            .next()
            .and_then(|line| line.split_whitespace().nth(1))
            .ok_or_else(|| io::Error::other("Bootstrap response status missing"))?
            .parse()
            .map_err(|_| io::Error::other("Bootstrap response status invalid"))?;
        let body = bytes[header_end + 4..].to_vec();
        remaining(deadline)?;
        Ok((status, body))
    }

    fn status(&mut self) -> Option<ExitStatus> {
        if self.exit.is_none() {
            self.exit = self
                .child
                .try_wait()
                .expect("Community bootstrap status failed");
        }
        self.exit
    }

    pub fn signal_term(&mut self) {
        assert!(
            self.status().is_none(),
            "Community exited before bootstrap SIGTERM"
        );
        let pid: libc::pid_t = self.child.id().try_into().unwrap();
        assert_eq!(
            unsafe { libc::kill(pid, libc::SIGTERM) },
            0,
            "Owned bootstrap SIGTERM failed"
        );
    }

    pub fn wait_exit(&mut self) -> ExitStatus {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Some(status) = self.status() {
                return status;
            }
            assert!(
                Instant::now() < deadline,
                "Bootstrap exit exceeded its deadline"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    pub fn events(&self, forbidden: &[&str]) -> Vec<Value> {
        let mut output = String::new();
        File::open(&self.log)
            .unwrap()
            .take(1_048_577)
            .read_to_string(&mut output)
            .unwrap();
        assert!(
            output.len() <= 1_048_576,
            "Bootstrap log capture exceeded its bound"
        );
        for &private in forbidden {
            assert!(
                !output.contains(private),
                "Bootstrap logs exposed private fixture input"
            );
        }
        let events: Vec<Value> = output
            .lines()
            .map(|line| serde_json::from_str(line).expect("Bootstrap logs must be structured JSON"))
            .collect();
        assert!(!events.is_empty(), "Bootstrap lifecycle logs missing");
        for event in &events {
            let target = event["target"]
                .as_str()
                .expect("Bootstrap log target missing");
            assert!(
                target == "hito_community" || target.starts_with("hito_community::"),
                "Dependency log escaped the target filter"
            );
        }
        events
    }
}

fn remaining(deadline: Instant) -> io::Result<Duration> {
    let remaining = deadline.saturating_duration_since(Instant::now());
    if remaining.is_zero() {
        Err(io::Error::new(
            io::ErrorKind::TimedOut,
            "Bootstrap request deadline exceeded",
        ))
    } else {
        Ok(remaining)
    }
}

impl Drop for Process {
    fn drop(&mut self) {
        if self.exit.is_none() {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}
