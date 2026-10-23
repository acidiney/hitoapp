use std::{
    fs::File,
    io::{self, Read, Write},
    net::{SocketAddr, TcpListener, TcpStream},
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
    pub fn start(directory: &Path, credential: &Path) -> Self {
        let reservation =
            TcpListener::bind("127.0.0.1:0").expect("runtime port reservation failed");
        let address = reservation
            .local_addr()
            .expect("runtime port address failed");
        drop(reservation);
        let log = directory.join("runtime.jsonl");
        let output = File::create(&log).expect("runtime capture setup failed");
        let child = Command::new(env!("CARGO_BIN_EXE_hito-plugin-jamendo"))
            .arg("--client-id-file")
            .arg(credential)
            .arg("--listen")
            .arg(address.to_string())
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

    pub fn manifest(&mut self) -> (u16, Vec<u8>) {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Ok(response) = self.request(
                "GET",
                "/v1/manifest?private-uri-query=private-uri-value",
                "",
            ) {
                return response;
            }
            assert!(
                self.status().is_none(),
                "runtime exited before real TCP manifest readiness"
            );
            assert!(
                Instant::now() < deadline,
                "runtime manifest readiness timed out"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    pub fn request(&self, method: &str, path: &str, payload: &str) -> io::Result<(u16, Vec<u8>)> {
        let mut connection = connection(self.address)?;
        write!(
            connection,
            "{method} {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\nContent-Type: application/json\r\nAuthorization: Bearer private-runtime-bearer\r\nCookie: private-runtime-cookie\r\nContent-Length: {}\r\n\r\n{payload}",
            payload.len()
        )?;
        response(connection)
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

    pub fn wait_event(&mut self, event: &str) {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if self.logs().contains(event) {
                return;
            }
            assert!(
                self.status().is_none(),
                "runtime exited before its lifecycle event"
            );
            assert!(
                Instant::now() < deadline,
                "runtime lifecycle event timed out"
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
                "runtime graceful shutdown timed out"
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
            let deadline = Instant::now() + Duration::from_secs(2);
            while Instant::now() < deadline {
                if matches!(self.child.try_wait(), Ok(Some(_))) {
                    break;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
        }
    }
}

pub struct PendingRequest(TcpStream);

impl PendingRequest {
    pub fn start(address: SocketAddr, payload: &str) -> Self {
        let mut connection = connection(address).expect("runtime drain connection failed");
        write!(connection,
            "POST /v1/search?private-drain-query=private-value HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\nContent-Type: application/json\r\nExpect: 100-continue\r\nAuthorization: Bearer private-drain-bearer\r\nCookie: private-drain-cookie\r\nContent-Length: {}\r\n\r\n",
            payload.len()).expect("runtime drain headers failed");
        let mut interim = Vec::new();
        while !interim.ends_with(b"\r\n\r\n") {
            let mut byte = [0];
            connection
                .read_exact(&mut byte)
                .expect("runtime did not accept the pending request body");
            interim.push(byte[0]);
            assert!(
                interim.len() <= 4096,
                "runtime interim headers exceeded their bound"
            );
        }
        assert!(
            interim.starts_with(b"HTTP/1.1 100"),
            "runtime did not start reading the pending body"
        );
        Self(connection)
    }

    pub fn finish(&mut self, payload: &str) -> io::Result<(u16, Vec<u8>)> {
        self.0.write_all(payload.as_bytes())?;
        response(&mut self.0)
    }
}

fn connection(address: SocketAddr) -> io::Result<TcpStream> {
    let connection = TcpStream::connect_timeout(&address, Duration::from_millis(100))?;
    connection.set_read_timeout(Some(Duration::from_secs(4)))?;
    connection.set_write_timeout(Some(Duration::from_secs(1)))?;
    Ok(connection)
}

fn response(connection: impl Read) -> io::Result<(u16, Vec<u8>)> {
    let mut bytes = Vec::new();
    connection.take(73_729).read_to_end(&mut bytes)?;
    if bytes.len() > 73_728 {
        return Err(io::Error::other("runtime response exceeded its bound"));
    }
    let offset = bytes
        .windows(4)
        .position(|part| part == b"\r\n\r\n")
        .ok_or_else(|| io::Error::other("runtime response headers missing"))?
        + 4;
    let status = std::str::from_utf8(
        bytes
            .get(9..12)
            .ok_or_else(|| io::Error::other("runtime response status missing"))?,
    )
    .map_err(|_| io::Error::other("runtime response status invalid"))?
    .parse()
    .map_err(|_| io::Error::other("runtime response status invalid"))?;
    Ok((status, bytes[offset..].to_vec()))
}
