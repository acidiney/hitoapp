#![cfg(all(target_os = "linux", not(target_env = "uclibc")))]

use hito_plugin_sdk::{Capability, Manifest, SearchPage};
use nix::{
    sys::signal::{Signal, kill},
    unistd::Pid,
};
use std::{
    collections::BTreeSet,
    fs::{self, File},
    io::{self, Read, Write},
    net::{SocketAddr, TcpListener, TcpStream},
    os::unix::fs::PermissionsExt,
    path::Path,
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

const SOURCE_ID: &str = "youtube:v1:HitoTone001:313430";

#[test]
fn metadata_only_mode_drops_sources_and_never_starts_the_extractor_for_them() {
    let directory = tempfile::tempdir().unwrap();
    let executable = directory.path().join("owned-yt-dlp");
    fs::write(&executable, include_str!("../src/fixtures/search.sh")).unwrap();
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
    let mut host = Host::start(directory.path(), &executable, &["--metadata-only"]);
    let (status, body) = host.wait_for("GET", "/v1/manifest", "");
    assert_eq!(status, 200);
    let manifest: Manifest = serde_json::from_slice(&body).unwrap();
    manifest.validate().unwrap();
    assert_eq!(
        manifest.capabilities,
        BTreeSet::from([Capability::Search, Capability::Metadata])
    );

    let recording = r#"{"recording_id":"HitoTone001"}"#;
    let (status, _) = http(host.address, "POST", "/v1/sources", recording).unwrap();
    assert_eq!(status, 404);
    let audio = format!("/v1/audio?source_id={SOURCE_ID}");
    for method in ["GET", "HEAD"] {
        let (status, _) = http(host.address, method, &audio, "").unwrap();
        assert_eq!(status, 404, "{method} audio must be refused");
    }
    assert!(
        !directory.path().join("argv").exists(),
        "Refused routes must not start the extractor"
    );

    let payload = r#"{"query":"owned tone","cursor":null,"limit":1}"#;
    let (status, body) = http(host.address, "POST", "/v1/search", payload).unwrap();
    assert_eq!(status, 200);
    let page: SearchPage = serde_json::from_slice(&body).unwrap();
    page.validate(1).unwrap();
    assert_eq!(page.items.len(), 1);
    assert_eq!(
        fs::read_to_string(directory.path().join("argv"))
            .unwrap()
            .matches("invocation\n")
            .count(),
        1,
        "Only the search may start the extractor"
    );

    host.stop();
    let logs = fs::read_to_string(directory.path().join("host.jsonl")).unwrap();
    let mode = logs
        .lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .find(|entry| entry["fields"]["event"] == "youtube_service_mode")
        .expect("Startup must log its mode");
    assert_eq!(mode["fields"]["mode"], "metadata_only");
    assert!(!logs.contains(directory.path().to_str().unwrap()));
}

#[test]
fn default_mode_keeps_sources_and_logs_its_mode() {
    let directory = tempfile::tempdir().unwrap();
    let executable = directory.path().join("owned-yt-dlp");
    fs::write(&executable, include_str!("../src/fixtures/search.sh")).unwrap();
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
    let mut host = Host::start(directory.path(), &executable, &[]);
    let (status, body) = host.wait_for("GET", "/v1/manifest", "");
    assert_eq!(status, 200);
    let manifest: Manifest = serde_json::from_slice(&body).unwrap();
    assert_eq!(
        manifest.capabilities,
        BTreeSet::from([
            Capability::Search,
            Capability::Sources,
            Capability::Metadata
        ])
    );
    let (_, body) = http(host.address, "GET", "/v1/audio?source_id=short", "").unwrap();
    assert!(
        String::from_utf8_lossy(&body).contains("YouTube source was not found"),
        "Default mode must keep the audio route"
    );
    host.stop();
    let logs = fs::read_to_string(directory.path().join("host.jsonl")).unwrap();
    assert!(logs.contains("\"mode\":\"full\""));
}

struct Host {
    child: Child,
    address: SocketAddr,
}

impl Host {
    fn start(directory: &Path, executable: &Path, extra: &[&str]) -> Self {
        let reservation = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = reservation.local_addr().unwrap();
        drop(reservation);
        let output = File::create(directory.join("host.jsonl")).unwrap();
        let child = Command::new(env!("CARGO_BIN_EXE_hito-plugin-youtube"))
            .arg("--listen")
            .arg(address.to_string())
            .arg("--executable")
            .arg(executable)
            .args(extra)
            .env("RUST_LOG", "info")
            .stdout(Stdio::from(output.try_clone().unwrap()))
            .stderr(Stdio::from(output))
            .spawn()
            .unwrap();
        Self { child, address }
    }

    fn wait_for(&mut self, method: &str, path: &str, payload: &str) -> (u16, Vec<u8>) {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Ok(response) = http(self.address, method, path, payload) {
                return response;
            }
            assert!(
                self.child.try_wait().unwrap().is_none(),
                "Host exited before readiness"
            );
            assert!(Instant::now() < deadline, "Host did not become ready");
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    fn stop(&mut self) {
        let pid = i32::try_from(self.child.id()).unwrap();
        kill(Pid::from_raw(pid), Signal::SIGTERM).unwrap();
        assert!(self.child.wait().unwrap().success());
    }
}

impl Drop for Host {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn http(
    address: SocketAddr,
    method: &str,
    path: &str,
    payload: &str,
) -> io::Result<(u16, Vec<u8>)> {
    let mut stream = TcpStream::connect_timeout(&address, Duration::from_millis(200))?;
    stream.set_read_timeout(Some(Duration::from_secs(4)))?;
    write!(
        stream,
        "{method} {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{payload}",
        payload.len()
    )?;
    let mut bytes = Vec::new();
    stream.take(73_729).read_to_end(&mut bytes)?;
    let offset = bytes
        .windows(4)
        .position(|part| part == b"\r\n\r\n")
        .ok_or_else(|| io::Error::other("Missing HTTP response headers"))?
        + 4;
    let status = std::str::from_utf8(&bytes[9..12])
        .unwrap()
        .parse()
        .map_err(|_| io::Error::other("Invalid HTTP status"))?;
    Ok((status, bytes[offset..].to_vec()))
}
