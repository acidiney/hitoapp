#![cfg(target_os = "linux")]

#[path = "support/runtime_bootstrap.rs"]
mod runtime;

use runtime::Process;
use serde_json::Value;
use std::{
    ffi::CString,
    fs::{OpenOptions, Permissions},
    io::Write,
    net::TcpListener,
    os::unix::{
        ffi::OsStrExt,
        fs::{FileTypeExt, MetadataExt, OpenOptionsExt, PermissionsExt, symlink},
    },
    path::{Path, PathBuf},
};

const ADMIN_TOKEN: &str = "17a0427d104d87ad8adce74f1fbbe334cb4969d697d0fe4d00bc58e15863a5ad";
const INVALID_FILTER: &str = "trace,private-bootstrap-filter=not-a-level";

#[test]
fn unsafe_credentials_fail_before_state_creation_without_modifying_inputs() {
    for denial in [
        Denial::SymbolicLink,
        Denial::HardLink,
        Denial::ExposedPermissions,
        Denial::Fifo,
        Denial::Oversized,
        Denial::InvalidToken,
    ] {
        let fixture = Fixture::new();
        let snapshot = denial.prepare(&fixture);
        let occupied = TcpListener::bind("127.0.0.1:0").unwrap();
        let mut process = Process::start(
            fixture.directory.path(),
            &fixture.state,
            &fixture.credential,
            occupied.local_addr().unwrap(),
            "trace",
            "denied",
        );
        assert_eq!(
            process.wait_exit().code(),
            Some(1),
            "{denial:?}: bootstrap accepted"
        );
        assert!(
            !fixture.state.try_exists().unwrap(),
            "{denial:?}: state was created"
        );
        let mut forbidden = vec![
            ADMIN_TOKEN,
            "private-",
            fixture.directory.path().to_str().unwrap(),
        ];
        snapshot.assert_unchanged();
        for entry in &snapshot.entries {
            if let Some(bytes) = &entry.bytes {
                forbidden.push(std::str::from_utf8(bytes).unwrap().trim_end());
            }
        }
        if matches!(denial, Denial::Fifo) {
            assert!(
                std::fs::symlink_metadata(&fixture.credential)
                    .unwrap()
                    .file_type()
                    .is_fifo()
            );
        }
        let events = process.events(&forbidden);
        assert_no_readiness(&events);
        let failure = event(&events, "community_service_failed");
        assert!(
            failure["fields"]["reason"] == denial.reason(),
            "{denial:?}: wrong static reason"
        );
        assert!(failure["fields"]["outcome"] == "failed");
    }
}

#[test]
fn malformed_log_filter_falls_back_without_exposing_filter_or_request_inputs() {
    let fixture = Fixture::new();
    fixture.write_credential(format!("{ADMIN_TOKEN}\n").as_bytes());
    let credential = CredentialSnapshot::capture(std::slice::from_ref(&fixture.credential));
    let reservation = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = reservation.local_addr().unwrap();
    drop(reservation);
    let mut process = Process::start(
        fixture.directory.path(),
        &fixture.state,
        &fixture.credential,
        address,
        INVALID_FILTER,
        "invalid-filter",
    );
    process.ready();
    let (status, body) = process
        .request(
            "POST",
            "/v1/admin/reporters",
            Some(ADMIN_TOKEN),
            r#"{"private-bootstrap-body":"private-bootstrap-value"}"#,
        )
        .unwrap();
    assert_eq!(status, 422);
    assert!(serde_json::from_slice::<Value>(&body).unwrap()["error"] == "invalid_input");
    process.signal_term();
    assert!(process.wait_exit().success());
    credential.assert_unchanged();
    let events = process.events(&[
        ADMIN_TOKEN,
        INVALID_FILTER,
        "private-",
        fixture.directory.path().to_str().unwrap(),
    ]);
    event(&events, "community_service_ready");
    event(&events, "community_service_state_closed");
    event(&events, "community_service_stopped");
    assert!(
        events
            .iter()
            .all(|event| event["level"] != "TRACE" && event["level"] != "DEBUG")
    );
}

#[test]
fn occupied_listener_failure_closes_state_before_same_state_restart() {
    let fixture = Fixture::new();
    fixture.write_credential(format!("{ADMIN_TOKEN}\n").as_bytes());
    let credential = CredentialSnapshot::capture(std::slice::from_ref(&fixture.credential));
    let occupied = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = occupied.local_addr().unwrap();
    let mut failed = Process::start(
        fixture.directory.path(),
        &fixture.state,
        &fixture.credential,
        address,
        "trace",
        "bind-failed",
    );
    assert_eq!(failed.wait_exit().code(), Some(1));
    credential.assert_unchanged();
    let events = failed.events(&[
        ADMIN_TOKEN,
        "private-",
        fixture.directory.path().to_str().unwrap(),
    ]);
    assert_no_readiness(&events);
    assert!(event(&events, "community_service_failed")["fields"]["reason"] == "bind_failed");
    let index = |name: &str| {
        events
            .iter()
            .position(|event| event["fields"]["event"] == name)
            .unwrap()
    };
    assert!(index("community_state_closed") < index("community_service_state_closed"));
    assert!(index("community_service_state_closed") < index("community_service_failed"));
    assert!(
        !events
            .iter()
            .any(|event| event["fields"]["event"] == "community_service_stopped")
    );
    drop(occupied);

    let mut restarted = Process::start(
        fixture.directory.path(),
        &fixture.state,
        &fixture.credential,
        address,
        "trace",
        "bind-restarted",
    );
    restarted.ready();
    restarted.signal_term();
    assert!(restarted.wait_exit().success());
    credential.assert_unchanged();
    let events = restarted.events(&[
        ADMIN_TOKEN,
        "private-",
        fixture.directory.path().to_str().unwrap(),
    ]);
    event(&events, "community_service_ready");
    event(&events, "community_service_state_closed");
    event(&events, "community_service_stopped");
}

#[derive(Clone, Copy, Debug)]
enum Denial {
    SymbolicLink,
    HardLink,
    ExposedPermissions,
    Fifo,
    Oversized,
    InvalidToken,
}

impl Denial {
    fn prepare(self, fixture: &Fixture) -> CredentialSnapshot {
        let mut paths = vec![fixture.credential.clone()];
        if matches!(self, Self::Fifo) {
            let path = CString::new(fixture.credential.as_os_str().as_bytes()).unwrap();
            assert_eq!(unsafe { libc::mkfifo(path.as_ptr(), 0o600) }, 0);
            return CredentialSnapshot::capture(&paths);
        }
        let bytes = match self {
            Self::Oversized => ADMIN_TOKEN.repeat(1024).into_bytes(),
            Self::InvalidToken => format!("{}g\n", &ADMIN_TOKEN[..63]).into_bytes(),
            _ => format!("{ADMIN_TOKEN}\n").into_bytes(),
        };
        fixture.write_credential(&bytes);
        match self {
            Self::SymbolicLink => {
                let target = fixture.directory.path().join("private-admin-target");
                std::fs::rename(&fixture.credential, &target).unwrap();
                symlink(&target, &fixture.credential).unwrap();
                paths.push(target);
            }
            Self::HardLink => {
                let alias = fixture.directory.path().join("private-admin-alias");
                std::fs::hard_link(&fixture.credential, &alias).unwrap();
                paths.push(alias);
            }
            Self::ExposedPermissions => {
                std::fs::set_permissions(&fixture.credential, Permissions::from_mode(0o644))
                    .unwrap();
            }
            _ => {}
        }
        CredentialSnapshot::capture(&paths)
    }

    fn reason(self) -> &'static str {
        match self {
            Self::SymbolicLink => "admin_token_file_unavailable",
            Self::InvalidToken => "admin_token_file_invalid",
            _ => "admin_token_file_not_private",
        }
    }
}

struct Fixture {
    directory: tempfile::TempDir,
    state: PathBuf,
    credential: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        assert_eq!(ADMIN_TOKEN.len(), 64);
        let directory = tempfile::tempdir().unwrap();
        std::fs::set_permissions(directory.path(), Permissions::from_mode(0o700)).unwrap();
        let state = directory.path().join("private-community-state");
        let credential = directory.path().join("private-admin-token");
        Self {
            directory,
            state,
            credential,
        }
    }

    fn write_credential(&self, bytes: &[u8]) {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&self.credential)
            .unwrap();
        file.write_all(bytes).unwrap();
    }
}

struct CredentialSnapshot {
    entries: Vec<EntrySnapshot>,
}

impl CredentialSnapshot {
    fn capture(paths: &[PathBuf]) -> Self {
        Self {
            entries: paths
                .iter()
                .map(|path| EntrySnapshot::capture(path))
                .collect(),
        }
    }

    fn assert_unchanged(&self) {
        for entry in &self.entries {
            entry.assert_unchanged();
        }
    }
}

struct EntrySnapshot {
    path: PathBuf,
    identity: EntryIdentity,
    link_target: Option<PathBuf>,
    bytes: Option<Vec<u8>>,
}

impl EntrySnapshot {
    fn capture(path: &Path) -> Self {
        let metadata = std::fs::symlink_metadata(path).unwrap();
        let link_target = metadata
            .file_type()
            .is_symlink()
            .then(|| std::fs::read_link(path).unwrap());
        let bytes = metadata.is_file().then(|| std::fs::read(path).unwrap());
        Self {
            path: path.to_owned(),
            identity: EntryIdentity::of(&metadata),
            link_target,
            bytes,
        }
    }

    fn assert_unchanged(&self) {
        let metadata = std::fs::symlink_metadata(&self.path).unwrap();
        assert!(
            EntryIdentity::of(&metadata) == self.identity,
            "Credential entry metadata changed"
        );
        if let Some(target) = &self.link_target {
            assert!(
                std::fs::read_link(&self.path).unwrap() == *target,
                "Credential symlink target changed"
            );
        }
        if let Some(bytes) = &self.bytes {
            assert!(
                std::fs::read(&self.path).unwrap() == *bytes,
                "Credential bytes changed"
            );
        }
    }
}

#[derive(PartialEq, Eq)]
struct EntryIdentity {
    device: u64,
    inode: u64,
    mode: u32,
    owner: u32,
    group: u32,
    links: u64,
    length: u64,
}

impl EntryIdentity {
    fn of(metadata: &std::fs::Metadata) -> Self {
        Self {
            device: metadata.dev(),
            inode: metadata.ino(),
            mode: metadata.mode(),
            owner: metadata.uid(),
            group: metadata.gid(),
            links: metadata.nlink(),
            length: metadata.len(),
        }
    }
}

fn event<'a>(events: &'a [Value], name: &str) -> &'a Value {
    events
        .iter()
        .find(|event| event["fields"]["event"] == name)
        .expect("Community lifecycle event missing")
}

fn assert_no_readiness(events: &[Value]) {
    assert!(
        !events
            .iter()
            .any(|event| event["fields"]["event"] == "community_service_ready")
    );
}
