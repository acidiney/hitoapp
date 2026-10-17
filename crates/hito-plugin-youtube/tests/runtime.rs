#![cfg(all(target_os = "linux", not(target_env = "uclibc")))]

use hito_plugin_sdk::{Manifest, SearchPage};
use nix::{
    sys::signal::{Signal, kill},
    unistd::Pid,
};
use std::{
    collections::BTreeMap,
    fs::{self, File},
    io::{self, Read, Write},
    net::{SocketAddr, TcpListener, TcpStream},
    os::unix::fs::PermissionsExt,
    path::Path,
    process::{Child, Command, ExitStatus, Stdio},
    time::{Duration, Instant},
};

#[test]
fn binary_search_supplies_only_the_owned_runtime_environment() {
    let directory = tempfile::tempdir().unwrap();
    let executable = directory.path().join("private-executable-path-sentinel");
    fs::write(&executable, include_str!("fixtures/runtime.sh")).unwrap();
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
    let mut host = Host::start(directory.path(), &executable);
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        if let Ok((200, body)) = http(host.address, "GET", "/v1/manifest", "") {
            let manifest: Manifest = serde_json::from_slice(&body).unwrap();
            manifest.validate().unwrap();
            assert_eq!(manifest.id, "org.hito.youtube");
            break;
        }
        assert!(host.status().is_none(), "Host exited before TCP readiness");
        assert!(
            Instant::now() < deadline,
            "Host did not publish its manifest"
        );
        std::thread::sleep(Duration::from_millis(10));
    }

    let query = "private-search-query-sentinel; $(printf private-shell-sentinel)";
    let payload = serde_json::json!({"query":query,"cursor":null,"limit":1}).to_string();
    let (status, body) = http(host.address, "POST", "/v1/search", &payload).unwrap();
    assert_eq!(status, 200);
    let page: SearchPage = serde_json::from_slice(&body).unwrap();
    page.validate(1).unwrap();
    assert_eq!(page.items.len(), 1);
    assert_eq!(page.items[0].id, "HitoTone001");
    assert_eq!(page.items[0].title, "Owned runtime tone");
    assert!(host.stop().success(), "Host did not join a clean shutdown");

    let environment: BTreeMap<_, _> = records(&directory.path().join("environment"))
        .into_iter()
        .map(|entry| {
            let (key, value) = entry.split_once('=').unwrap();
            (key.to_owned(), value.to_owned())
        })
        .collect();
    let expected: BTreeMap<_, _> = [
        ("PATH", "/usr/local/bin:/usr/bin:/bin"),
        ("LANG", "C.UTF-8"),
        ("YTDLP_NO_PLUGINS", "1"),
        ("DENO_DIR", "/tmp/deno"),
        ("DENO_NO_UPDATE_CHECK", "1"),
        ("DENO_NO_PACKAGE_JSON", "1"),
        ("TMPDIR", "/tmp"),
    ]
    .into_iter()
    .map(|(key, value)| (key.to_owned(), value.to_owned()))
    .collect();
    assert_eq!(environment, expected);

    let arguments = records(&directory.path().join("arguments"));
    let mut expected_query = url::Url::parse("https://music.youtube.com/search").unwrap();
    expected_query.query_pairs_mut().append_pair("q", query);
    expected_query.set_fragment(Some("songs"));
    let expected_arguments: Vec<_> = [
        "--ignore-config",
        "--no-config-locations",
        "--simulate",
        "--skip-download",
        "--no-cookies",
        "--no-cookies-from-browser",
        "--no-plugin-dirs",
        "--no-remote-components",
        "--no-cache-dir",
        "--dump-single-json",
        "--flat-playlist",
        "--playlist-end",
        "1",
        "--",
    ]
    .into_iter()
    .map(str::to_owned)
    .chain([expected_query.to_string()])
    .collect();
    assert!(
        arguments == expected_arguments,
        "the owned runtime must receive the exact encoded Music search process arguments"
    );

    let mut logs = String::new();
    File::open(directory.path().join("host.jsonl"))
        .unwrap()
        .take(1_048_577)
        .read_to_string(&mut logs)
        .unwrap();
    assert!(
        logs.len() <= 1_048_576,
        "Owned host logs exceeded their bound"
    );
    assert!(!logs.contains("private-"), "Logs exposed private input");
    assert!(
        !logs.contains(directory.path().to_str().unwrap()),
        "Logs exposed an owned private path"
    );
    assert!(logs.contains("youtube_search_complete"));
    assert!(logs.contains("youtube_shutdown_complete"));
    assert!(logs.contains("youtube_service_stopped"));
}

fn records(path: &Path) -> Vec<String> {
    let mut bytes = Vec::new();
    File::open(path)
        .unwrap()
        .take(65_537)
        .read_to_end(&mut bytes)
        .unwrap();
    assert!(
        bytes.len() <= 65_536,
        "Owned process trace exceeded its bound"
    );
    assert_eq!(bytes.pop(), Some(0));
    bytes
        .split(|byte| *byte == 0)
        .map(|value| std::str::from_utf8(value).unwrap().to_owned())
        .collect()
}

struct Host {
    child: Child,
    exit: Option<ExitStatus>,
    address: SocketAddr,
}

impl Host {
    fn start(directory: &Path, executable: &Path) -> Self {
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
            .env("RUST_LOG", "trace")
            .stdout(Stdio::from(output.try_clone().unwrap()))
            .stderr(Stdio::from(output));
        for key in [
            "HOME",
            "PATH",
            "LANG",
            "XDG_CONFIG_HOME",
            "XDG_CACHE_HOME",
            "HTTP_PROXY",
            "HTTPS_PROXY",
            "ALL_PROXY",
            "NO_PROXY",
            "YTDLP_NO_PLUGINS",
            "YTDLP_PLUGIN_DIR",
            "DENO_DIR",
            "DENO_NO_UPDATE_CHECK",
            "DENO_NO_PACKAGE_JSON",
            "TMPDIR",
            "PRIVATE_BROWSER_COOKIE",
        ] {
            command.env(key, "private-environment-sentinel");
        }
        Self {
            child: command.spawn().unwrap(),
            exit: None,
            address,
        }
    }

    fn status(&mut self) -> Option<ExitStatus> {
        if self.exit.is_none() {
            self.exit = self.child.try_wait().unwrap();
        }
        self.exit
    }

    fn stop(&mut self) -> ExitStatus {
        assert!(self.status().is_none(), "Host exited before owned SIGTERM");
        let pid = i32::try_from(self.child.id()).unwrap();
        kill(Pid::from_raw(pid), Signal::SIGTERM).unwrap();
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            if let Some(status) = self.status() {
                return status;
            }
            assert!(Instant::now() < deadline, "Host did not finish shutdown");
            std::thread::sleep(Duration::from_millis(10));
        }
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
        "{method} {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\nContent-Type: application/json\r\nAuthorization: Bearer private-bearer-sentinel\r\nCookie: private-cookie-sentinel\r\nContent-Length: {}\r\n\r\n{payload}",
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
