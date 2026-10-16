use std::{
    fs::{self, File},
    io::{Read, Seek, Write},
    net::{SocketAddr, TcpListener, TcpStream},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

pub struct Service {
    child: Child,
    pub address: SocketAddr,
    log: PathBuf,
}

pub fn initialize(music: &Path, state: &Path) {
    let mut output = tempfile::tempfile().unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_hito-plugin-folder"))
        .arg("--music")
        .arg(music)
        .arg("--state")
        .arg(state)
        .arg("--initialize-state")
        .env("RUST_LOG", "trace")
        .stdout(Stdio::from(output.try_clone().unwrap()))
        .stderr(Stdio::from(output.try_clone().unwrap()))
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            output.rewind().unwrap();
            let mut logs = String::new();
            output.take(65536).read_to_string(&mut logs).unwrap();
            assert!(
                status.success(),
                "explicit state initialization failed: {logs}"
            );
            return;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("state initialization did not exit");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

impl Service {
    fn spawn(music: &Path, state: &Path, log: &Path) -> Self {
        let reservation = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = reservation.local_addr().unwrap();
        drop(reservation);
        let output = File::create(log).unwrap();
        let child = Command::new(env!("CARGO_BIN_EXE_hito-plugin-folder"))
            .arg("--music")
            .arg(music)
            .arg("--state")
            .arg(state)
            .arg("--listen")
            .arg(address.to_string())
            .env("RUST_LOG", "trace")
            .stdout(Stdio::from(output.try_clone().unwrap()))
            .stderr(Stdio::from(output))
            .spawn()
            .unwrap();
        Self {
            child,
            address,
            log: log.into(),
        }
    }

    pub fn start(music: &Path, state: &Path, log: &Path) -> Self {
        let mut service = Self::spawn(music, state, log);
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let logs = fs::read_to_string(log).unwrap();
            if logs.contains("folder_service_ready") {
                return service;
            }
            assert!(
                service.child.try_wait().unwrap().is_none(),
                "service exited before readiness: {logs}"
            );
            assert!(Instant::now() < deadline, "service startup timed out");
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    #[allow(dead_code)]
    pub fn reject_start(music: &Path, state: &Path, log: &Path) {
        let mut service = Self::spawn(music, state, log);
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let status = service.child.try_wait().unwrap();
            let logs = fs::read_to_string(log).unwrap();
            assert!(
                !logs.contains("folder_service_ready"),
                "failed startup published HTTP: {logs}"
            );
            assert!(
                !logs.contains("folder_registry_committed"),
                "failed startup committed a partial generation: {logs}"
            );
            if let Some(status) = status {
                assert_eq!(status.code(), Some(1), "unexpected startup exit: {logs}");
                assert!(
                    logs.contains("folder_service_failed"),
                    "missing failure event: {logs}"
                );
                assert!(
                    logs.contains("startup_failed"),
                    "missing failure outcome: {logs}"
                );
                return;
            }
            assert!(Instant::now() < deadline, "rejected startup did not exit");
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    pub fn post<T: serde::de::DeserializeOwned>(&self, path: &str, body: serde_json::Value) -> T {
        let body = body.to_string();
        let (status, body) = self.request("POST", path, &body, None);
        assert_eq!(status, 200);
        serde_json::from_slice(&body).unwrap()
    }

    #[allow(dead_code)]
    pub fn post_status(&self, path: &str, body: serde_json::Value) -> u16 {
        self.request("POST", path, &body.to_string(), None).0
    }

    pub fn audio(&self, source_id: &str, first: usize, end: usize) -> (u16, Vec<u8>) {
        assert!(first < end);
        let encoded = source_id
            .bytes()
            .map(|byte| format!("%{byte:02X}"))
            .collect::<String>();
        self.request(
            "GET",
            &format!("/v1/audio?source_id={encoded}"),
            "",
            Some(&format!("bytes={first}-{}", end - 1)),
        )
    }

    fn request(&self, method: &str, path: &str, body: &str, range: Option<&str>) -> (u16, Vec<u8>) {
        let mut connection = TcpStream::connect(self.address).unwrap();
        connection
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        write!(connection,"{method} {path} HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n",body.len()).unwrap();
        if let Some(range) = range {
            write!(connection, "Range: {range}\r\n").unwrap();
        }
        write!(connection, "\r\n{body}").unwrap();
        let mut response = Vec::new();
        connection.take(263168).read_to_end(&mut response).unwrap();
        let offset = response
            .windows(4)
            .position(|part| part == b"\r\n\r\n")
            .unwrap()
            + 4;
        let status = std::str::from_utf8(&response[9..12])
            .unwrap()
            .parse()
            .unwrap();
        (status, response[offset..].to_owned())
    }

    pub fn stop(mut self) {
        assert_eq!(
            unsafe { libc::kill(self.child.id() as libc::pid_t, libc::SIGTERM) },
            0
        );
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Some(status) = self.child.try_wait().unwrap() {
                assert!(status.success());
                assert!(
                    fs::read_to_string(&self.log)
                        .unwrap()
                        .contains("folder_service_stopped")
                );
                return;
            }
            assert!(Instant::now() < deadline, "service shutdown timed out");
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

impl Drop for Service {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
