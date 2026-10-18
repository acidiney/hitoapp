use serde_json::Value;
use std::{
    fs::{File, OpenOptions},
    io::{self, Read, Write},
    net::{SocketAddr, TcpListener, TcpStream},
    os::unix::fs::OpenOptionsExt,
    path::{Path, PathBuf},
    process::{Child, Command, ExitStatus, Stdio},
    time::{Duration, Instant},
};

pub struct Host {
    child: Child,
    exit: Option<ExitStatus>,
    pub address: SocketAddr,
    log: PathBuf,
}

impl Host {
    pub fn start(directory: &Path, provider: &Path, service: &Path, catalog: &Path) -> Self {
        let reservation =
            TcpListener::bind("127.0.0.1:0").expect("runtime port reservation failed");
        let address = reservation
            .local_addr()
            .expect("runtime port address failed");
        drop(reservation);
        let log = directory.join("runtime.jsonl");
        let output = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&log)
            .expect("runtime capture setup failed");
        let child = Command::new(env!("CARGO_BIN_EXE_hito-plugin-real-debrid"))
            .arg("--provider-token-file")
            .arg(provider)
            .arg("--service-token-file")
            .arg(service)
            .arg("--catalog-file")
            .arg(catalog)
            .arg("--listen")
            .arg(address.to_string())
            .env_clear()
            .env("RUST_LOG", "trace")
            .stdout(Stdio::from(
                output.try_clone().expect("runtime capture clone failed"),
            ))
            .stderr(Stdio::from(output))
            .spawn()
            .expect("runtime process spawn failed");
        Self {
            child,
            exit: None,
            address,
            log,
        }
    }

    pub fn request(
        &self,
        method: &str,
        path: &str,
        token: Option<&str>,
        payload: &str,
    ) -> io::Result<(u16, Vec<u8>)> {
        let mut stream = connection(self.address)?;
        write!(
            stream,
            "{method} {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\nContent-Type: application/json\r\nX-Hito-Plugin-Protocol: 1\r\nCookie: private-runtime-cookie\r\nX-Private-Header: private-runtime-header-value\r\n"
        )?;
        if let Some(token) = token {
            write!(stream, "Authorization: Bearer {token}\r\n")?;
        }
        write!(stream, "Content-Length: {}\r\n\r\n{payload}", payload.len())?;
        response(stream)
    }

    pub fn status(&mut self) -> Option<ExitStatus> {
        if self.exit.is_none() {
            self.exit = self
                .child
                .try_wait()
                .expect("runtime process status failed");
        }
        self.exit
    }

    pub fn signal_term(&mut self) {
        assert!(
            self.status().is_none(),
            "runtime exited before its owned signal"
        );
        let status = Command::new("/bin/sh")
            .args(["-c", "kill -TERM \"$1\"", "hito-owned-runtime-signal"])
            .arg(self.child.id().to_string())
            .status()
            .expect("owned runtime signal failed");
        assert!(status.success(), "owned runtime SIGTERM failed");
    }

    pub fn wait_event(&mut self, name: &str) {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if self
                .logs()
                .lines()
                .filter_map(|line| serde_json::from_str::<Value>(line).ok())
                .any(|event| event["fields"]["event"] == name)
            {
                return;
            }
            assert!(
                self.status().is_none(),
                "runtime exited before its lifecycle acknowledgement"
            );
            assert!(
                Instant::now() < deadline,
                "runtime lifecycle acknowledgement timed out"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    pub fn wait_exit(&mut self) -> ExitStatus {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Some(status) = self.status() {
                return status;
            }
            assert!(
                Instant::now() < deadline,
                "runtime joined shutdown timed out"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    pub fn logs(&self) -> String {
        let mut output = String::new();
        File::open(&self.log)
            .expect("runtime capture read failed")
            .take(1_048_577)
            .read_to_string(&mut output)
            .expect("runtime capture text failed");
        assert!(
            output.len() <= 1_048_576,
            "runtime capture exceeded its bound"
        );
        output
    }
}

impl Drop for Host {
    fn drop(&mut self) {
        if self.exit.is_none() {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

pub struct PendingRequest(TcpStream);

impl PendingRequest {
    pub fn start(address: SocketAddr, token: &str, payload: &str) -> Self {
        let mut stream = connection(address).expect("runtime drain connection failed");
        write!(stream, "POST /v1/search?private-runtime-query=private-runtime-query-value HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\nContent-Type: application/json\r\nX-Hito-Plugin-Protocol: 1\r\nExpect: 100-continue\r\nAuthorization: Bearer {token}\r\nCookie: private-runtime-cookie\r\nContent-Length: {}\r\n\r\n", payload.len()).expect("runtime drain headers failed");
        let mut interim = Vec::new();
        while !interim.ends_with(b"\r\n\r\n") {
            let mut byte = [0];
            stream
                .read_exact(&mut byte)
                .expect("runtime did not accept the held authenticated body");
            interim.push(byte[0]);
            assert!(
                interim.len() <= 4096,
                "runtime interim headers exceeded their bound"
            );
        }
        assert!(
            interim.starts_with(b"HTTP/1.1 100"),
            "runtime did not start reading the held authenticated body"
        );
        Self(stream)
    }

    pub fn finish(&mut self, payload: &str) -> io::Result<(u16, Vec<u8>)> {
        self.0.write_all(payload.as_bytes())?;
        response(&mut self.0)
    }
}

fn connection(address: SocketAddr) -> io::Result<TcpStream> {
    let stream = TcpStream::connect_timeout(&address, Duration::from_millis(100))?;
    stream.set_read_timeout(Some(Duration::from_secs(4)))?;
    stream.set_write_timeout(Some(Duration::from_secs(1)))?;
    Ok(stream)
}

fn response(stream: impl Read) -> io::Result<(u16, Vec<u8>)> {
    let mut bytes = Vec::new();
    stream.take(73_729).read_to_end(&mut bytes)?;
    if bytes.len() > 73_728 {
        return Err(io::Error::other("runtime response exceeded its bound"));
    }
    let header_end = bytes
        .windows(4)
        .position(|part| part == b"\r\n\r\n")
        .ok_or_else(|| io::Error::other("runtime response headers missing"))?;
    let headers = std::str::from_utf8(&bytes[..header_end])
        .map_err(|_| io::Error::other("runtime response headers invalid"))?;
    let status = headers
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .ok_or_else(|| io::Error::other("runtime response status missing"))?
        .parse()
        .map_err(|_| io::Error::other("runtime response status invalid"))?;
    Ok((status, bytes[header_end + 4..].to_vec()))
}
