use std::{
    fs::{File, OpenOptions},
    io::{self, Read, Write},
    net::{SocketAddr, TcpListener, TcpStream},
    os::unix::fs::OpenOptionsExt,
    path::{Path, PathBuf},
    process::{Child, Command, ExitStatus, Stdio},
    time::{Duration, Instant},
};

use serde_json::Value;

pub struct Host {
    child: Child,
    exit: Option<ExitStatus>,
    pub address: SocketAddr,
    log: PathBuf,
}

impl Host {
    pub fn start(directory: &Path, state: &Path, credential: &Path, run: &str) -> Self {
        let reservation = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = reservation.local_addr().unwrap();
        drop(reservation);
        let log = directory.join(format!("runtime-{run}.jsonl"));
        let output = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&log)
            .unwrap();
        let child = Command::new(env!("CARGO_BIN_EXE_hito-community"))
            .args(["--state-directory"])
            .arg(state)
            .args(["--admin-token-file"])
            .arg(credential)
            .args(["--listen"])
            .arg(address.to_string())
            .env_clear()
            .env("RUST_LOG", "trace")
            .stdout(Stdio::from(output.try_clone().unwrap()))
            .stderr(Stdio::from(output))
            .spawn()
            .expect("Community runtime spawn failed");
        Self {
            child,
            exit: None,
            address,
            log,
        }
    }

    pub fn ready(&mut self) -> Value {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Ok((status, body)) = self.request("GET", "/v1/openapi.json", None, "") {
                assert_eq!(status, 200);
                return serde_json::from_slice(&body).expect("Community readiness JSON failed");
            }
            assert!(
                self.status().is_none(),
                "Community exited before public OpenAPI readiness"
            );
            assert!(
                Instant::now() < deadline,
                "Community OpenAPI readiness exceeded its deadline"
            );
            std::thread::sleep(Duration::from_millis(10));
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
            "{method} {path}?private-community-query=private-query-value HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\nContent-Type: application/json\r\nCookie: private-community-cookie\r\nX-Private-Request: private-community-header\r\n"
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
                .expect("Community runtime status failed");
        }
        self.exit
    }

    pub fn signal_term(&mut self) {
        assert!(
            self.status().is_none(),
            "Community exited before its owned SIGTERM"
        );
        let pid: libc::pid_t = self.child.id().try_into().unwrap();
        assert_eq!(
            unsafe { libc::kill(pid, libc::SIGTERM) },
            0,
            "Owned community SIGTERM failed"
        );
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
                "Community exited before its lifecycle event"
            );
            assert!(
                Instant::now() < deadline,
                "Community lifecycle event exceeded its deadline"
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
                "Community joined shutdown exceeded its deadline"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    pub fn logs(&self) -> String {
        let mut output = String::new();
        File::open(&self.log)
            .unwrap()
            .take(1_048_577)
            .read_to_string(&mut output)
            .unwrap();
        assert!(
            output.len() <= 1_048_576,
            "Community logs exceeded the capture bound"
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

pub struct PendingProvision(TcpStream);

impl PendingProvision {
    pub fn start(address: SocketAddr, token: &str, payload: &str) -> Self {
        let mut stream = connection(address).expect("Community pending connection failed");
        write!(stream, "POST /v1/admin/reporters?private-community-query=private-query-value HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\nContent-Type: application/json\r\nAuthorization: Bearer {token}\r\nCookie: private-community-cookie\r\nX-Private-Request: private-community-header\r\nExpect: 100-continue\r\nContent-Length: {}\r\n\r\n", payload.len()).unwrap();
        let mut interim = Vec::new();
        while !interim.ends_with(b"\r\n\r\n") {
            let mut byte = [0];
            stream
                .read_exact(&mut byte)
                .expect("Community did not admit the held request body");
            interim.push(byte[0]);
            assert!(
                interim.len() <= 4096,
                "Community interim response exceeded its bound"
            );
        }
        assert!(
            interim.starts_with(b"HTTP/1.1 100"),
            "Community did not begin reading the held authenticated body"
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
    stream.take(262_145).read_to_end(&mut bytes)?;
    if bytes.len() > 262_144 {
        return Err(io::Error::other("Community response exceeded its bound"));
    }
    let header_end = bytes
        .windows(4)
        .position(|part| part == b"\r\n\r\n")
        .ok_or_else(|| io::Error::other("Community response headers missing"))?;
    let headers = std::str::from_utf8(&bytes[..header_end])
        .map_err(|_| io::Error::other("Community response headers invalid"))?;
    let status = headers
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .ok_or_else(|| io::Error::other("Community response status missing"))?
        .parse()
        .map_err(|_| io::Error::other("Community response status invalid"))?;
    Ok((status, bytes[header_end + 4..].to_vec()))
}
