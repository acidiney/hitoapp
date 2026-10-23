#![cfg(all(target_os = "linux", not(target_env = "uclibc")))]

use hito_plugin_sdk::Manifest;
use nix::{
    sys::signal::{Signal, kill},
    unistd::Pid,
};
use serde_json::Value;
use std::{
    fs::{self, File},
    io::{self, Read, Write},
    net::{SocketAddr, TcpListener, TcpStream},
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{Child, Command, ExitStatus, Stdio},
    time::{Duration, Instant},
};

#[test]
fn binary_sigterm_joins_active_extraction_before_runtime_exit_with_private_logs() {
    for trace in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let executable = directory.path().join("private-executable-path-sentinel");
        fs::write(&executable, include_str!("fixtures/host.sh")).unwrap();
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
        let mut host = Host::start(directory.path(), &executable, trace);
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            if let Ok((200, body)) = http(
                host.address,
                "GET",
                "/v1/manifest?private-uri-query-sentinel=private-uri-value-sentinel",
                "",
            ) {
                let manifest: Manifest = serde_json::from_slice(&body).unwrap();
                manifest.validate().unwrap();
                assert_eq!(manifest.id, "org.hito.youtube");
                break;
            }
            assert!(
                host.status().is_none(),
                "Host exited before real TCP manifest readiness"
            );
            assert!(
                Instant::now() < deadline,
                "Host did not publish a real TCP manifest"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        let address = host.address;
        let pending = std::thread::spawn(move || {
            http(
                address,
                "POST",
                "/v1/search?private-uri-query-sentinel=private-uri-value-sentinel",
                r#"{"query":"private-search-query-sentinel","cursor":null,"limit":1}"#,
            )
        });
        let deadline = Instant::now() + Duration::from_secs(3);
        while !directory.path().join("leader.ready").exists() {
            assert!(
                host.status().is_none(),
                "Host exited before owned extraction readiness"
            );
            assert!(
                Instant::now() < deadline,
                "Owned extraction did not become ready"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        let leader = Identity::read(&directory.path().join("leader.identity")).unwrap();
        let descendant = Identity::read(&directory.path().join("child.identity")).unwrap();
        assert_eq!(leader.group, leader.pid);
        assert_eq!(descendant.group, leader.pid);
        assert!(leader.running().unwrap());
        assert!(descendant.running().unwrap());
        assert!(!pending.is_finished());
        assert!(
            host.status().is_none(),
            "SIGTERM requires an owned unreaped host child"
        );
        let pid = i32::try_from(host.child.id()).unwrap();
        kill(Pid::from_raw(pid), Signal::SIGTERM).unwrap();

        let deadline = Instant::now() + Duration::from_secs(3);
        let status = loop {
            if let Some(status) = host.status() {
                break status;
            }
            assert!(
                Instant::now() < deadline,
                "Host did not finish joined shutdown"
            );
            std::thread::sleep(Duration::from_millis(10));
        };
        assert!(
            status.success(),
            "Owned host must exit successfully after SIGTERM"
        );
        assert!(
            !leader.present().unwrap(),
            "Host exited before reaping its extractor leader"
        );
        assert!(
            !descendant.running().unwrap(),
            "Host exited with an owned descendant still alive"
        );
        let (status, body) = pending.join().unwrap().unwrap();
        assert_eq!(status, 503);
        assert!(!std::str::from_utf8(&body).unwrap().contains("private-"));
        assert_eq!(
            serde_json::from_slice::<Value>(&body).unwrap(),
            serde_json::json!({"error":"The YouTube service is unavailable"})
        );

        let output = host.logs();
        assert!(
            !output.contains("private-"),
            "Production logs exposed injected private content"
        );
        assert!(
            !output.contains(directory.path().to_str().unwrap()),
            "Production logs exposed a private path"
        );
        assert!(
            !output.contains("https://audio.invalid/"),
            "Production logs exposed an upstream URL"
        );
        let events: Vec<Value> = output
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        let index = |name| {
            events
                .iter()
                .position(|event| event["fields"]["event"] == name)
                .expect("Missing production lifecycle event")
        };
        let ready = index("youtube_service_ready");
        let requested = index("youtube_shutdown_requested");
        let extracted = index("youtube_search_complete");
        let joined = index("youtube_shutdown_complete");
        let stopped = index("youtube_service_stopped");
        assert!(
            ready < requested && requested < extracted && extracted < joined && joined < stopped
        );
        assert_eq!(events[requested]["fields"]["signal"], "SIGTERM");
        assert_eq!(events[extracted]["fields"]["outcome"], "canceled");
        assert_eq!(events[joined]["fields"]["outcome"], "complete");
        assert_eq!(events[stopped]["fields"]["outcome"], "stopped");
        for event in [
            &events[ready],
            &events[extracted],
            &events[joined],
            &events[stopped],
        ] {
            assert_eq!(event["level"], "INFO");
            assert!(event["fields"]["elapsed_ms"].as_u64().is_some());
        }
        let request_id = correlation(&events[extracted]);
        assert!(events.iter().any(
            |event| event["fields"]["event"] == "youtube_request_complete"
                && event["fields"]["status"] == 503
                && correlation(event) == request_id
        ));
        if trace {
            let cleaned = index("youtube_process_stopped");
            assert!(requested < cleaned && cleaned < extracted);
            assert_eq!(events[cleaned]["fields"]["outcome"], "reaped");
            assert_eq!(events[cleaned]["fields"]["terminated"], true);
            assert_eq!(correlation(&events[cleaned]), request_id);
        }
    }
}

fn correlation(event: &Value) -> u64 {
    event["spans"]
        .as_array()
        .unwrap()
        .iter()
        .find(|span| span["name"] == "youtube_request")
        .and_then(|span| span["request_id"].as_u64())
        .expect("Extraction needs its numeric request correlation")
}

struct Host {
    child: Child,
    exit: Option<ExitStatus>,
    address: SocketAddr,
    directory: PathBuf,
}

impl Host {
    fn start(directory: &Path, executable: &Path, trace: bool) -> Self {
        let reservation = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = reservation.local_addr().unwrap();
        drop(reservation);
        let output = File::create(directory.join("host.jsonl")).unwrap();
        let mut command = Command::new(env!("CARGO_BIN_EXE_hito-plugin-youtube"));
        command
            .arg("--listen")
            .arg(address.to_string())
            .arg("--executable")
            .arg(executable)
            .env_remove("RUST_LOG")
            .stdout(Stdio::from(output.try_clone().unwrap()))
            .stderr(Stdio::from(output));
        if trace {
            command.env("RUST_LOG", "trace");
        }
        Self {
            child: command.spawn().unwrap(),
            exit: None,
            address,
            directory: directory.to_owned(),
        }
    }

    fn status(&mut self) -> Option<ExitStatus> {
        if self.exit.is_none() {
            self.exit = self.child.try_wait().unwrap();
        }
        self.exit
    }

    fn logs(&self) -> String {
        let mut output = String::new();
        File::open(self.directory.join("host.jsonl"))
            .unwrap()
            .take(1_048_577)
            .read_to_string(&mut output)
            .unwrap();
        assert!(output.len() <= 1_048_576);
        output
    }
}

impl Drop for Host {
    fn drop(&mut self) {
        let _ = fs::write(self.directory.join("release"), []);
        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline {
            let stopped = match (
                Identity::read(&self.directory.join("leader.identity")),
                Identity::read(&self.directory.join("child.identity")),
            ) {
                (Ok(leader), Ok(descendant)) => {
                    matches!(leader.present(), Ok(false))
                        && matches!(descendant.running(), Ok(false))
                }
                _ => self.status().is_some(),
            };
            if stopped {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        if self.status().is_none() {
            let _ = self.child.kill();
            let deadline = Instant::now() + Duration::from_secs(2);
            while self.status().is_none() && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(10));
            }
        }
    }
}

fn http(
    address: SocketAddr,
    method: &str,
    path: &str,
    payload: &str,
) -> io::Result<(u16, Vec<u8>)> {
    let mut stream = TcpStream::connect_timeout(&address, Duration::from_millis(100))?;
    stream.set_read_timeout(Some(Duration::from_secs(4)))?;
    stream.set_write_timeout(Some(Duration::from_secs(1)))?;
    write!(
        stream,
        "{method} {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\nContent-Type: application/json\r\nAuthorization: Bearer private-bearer-sentinel\r\nCookie: private-cookie-sentinel\r\nX-Owned-Private: private-header-sentinel\r\nContent-Length: {}\r\n\r\n{payload}",
        payload.len()
    )?;
    let mut bytes = Vec::new();
    stream.take(73_729).read_to_end(&mut bytes)?;
    if bytes.len() > 73_728 {
        return Err(io::Error::other("Owned HTTP response exceeded its bound"));
    }
    let offset = bytes
        .windows(4)
        .position(|part| part == b"\r\n\r\n")
        .ok_or_else(|| io::Error::other("Missing owned HTTP response headers"))?
        + 4;
    let status = std::str::from_utf8(
        bytes
            .get(9..12)
            .ok_or_else(|| io::Error::other("Missing owned HTTP status"))?,
    )
    .map_err(|_| io::Error::other("Invalid owned HTTP status"))?
    .parse()
    .map_err(|_| io::Error::other("Invalid owned HTTP status"))?;
    Ok((status, bytes[offset..].to_vec()))
}

struct Identity {
    pid: u32,
    group: u32,
    started: u64,
    state: char,
}

impl Identity {
    fn read(path: &Path) -> io::Result<Self> {
        let value = fs::read_to_string(path)?;
        let invalid = || io::Error::other("Invalid owned process identity");
        let pid = value
            .split_once(' ')
            .ok_or_else(invalid)?
            .0
            .parse()
            .map_err(|_| invalid())?;
        let fields: Vec<_> = value
            .rsplit_once(") ")
            .ok_or_else(invalid)?
            .1
            .split_whitespace()
            .collect();
        Ok(Self {
            pid,
            group: fields
                .get(2)
                .ok_or_else(invalid)?
                .parse()
                .map_err(|_| invalid())?,
            started: fields
                .get(19)
                .ok_or_else(invalid)?
                .parse()
                .map_err(|_| invalid())?,
            state: fields
                .first()
                .and_then(|field| field.chars().next())
                .ok_or_else(invalid)?,
        })
    }

    fn current(&self) -> io::Result<Option<Self>> {
        match Self::read(Path::new(&format!("/proc/{}/stat", self.pid))) {
            Ok(current) if current.pid == self.pid && current.started == self.started => {
                Ok(Some(current))
            }
            Ok(_) => Ok(None),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error),
        }
    }

    fn present(&self) -> io::Result<bool> {
        self.current().map(|current| current.is_some())
    }

    fn running(&self) -> io::Result<bool> {
        self.current()
            .map(|current| current.is_some_and(|current| !matches!(current.state, 'Z' | 'X')))
    }
}
