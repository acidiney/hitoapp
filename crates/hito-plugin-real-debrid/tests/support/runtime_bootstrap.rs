use serde_json::Value;
use std::{
    fs::{File, OpenOptions},
    io::Read,
    net::SocketAddr,
    os::{
        fd::{AsRawFd, FromRawFd, OwnedFd},
        unix::fs::OpenOptionsExt,
    },
    path::Path,
    process::{Child, Stdio},
    time::{Duration, Instant},
};

struct Process {
    child: Child,
    joined: bool,
}

impl Drop for Process {
    fn drop(&mut self) {
        if !self.joined {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

pub fn rejected(
    directory: &Path,
    provider: &Path,
    service: &Path,
    catalog: &Path,
    address: SocketAddr,
    reason: &str,
) {
    let log = directory.join("private-bootstrap-runtime.jsonl");
    let output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&log)
        .expect("bootstrap capture setup failed");
    let child = std::process::Command::new(env!("CARGO_BIN_EXE_hito-plugin-real-debrid"))
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
            output.try_clone().expect("bootstrap capture clone failed"),
        ))
        .stderr(Stdio::from(output))
        .spawn()
        .expect("bootstrap process spawn failed");
    let mut process = Process {
        child,
        joined: false,
    };
    let descriptor = unsafe { libc::syscall(libc::SYS_pidfd_open, process.child.id(), 0) };
    assert!(
        descriptor >= 0,
        "owned bootstrap process notification failed"
    );
    let notification = unsafe { OwnedFd::from_raw_fd(descriptor.try_into().unwrap()) };
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        assert!(
            !remaining.is_zero(),
            "bootstrap rejection exceeded its deadline"
        );
        let mut event = libc::pollfd {
            fd: notification.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        let ready = unsafe { libc::poll(&mut event, 1, remaining.as_millis().max(1) as i32) };
        if ready < 0 && std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted {
            continue;
        }
        assert!(
            ready == 1 && event.revents & libc::POLLIN != 0,
            "bootstrap rejection did not complete within its deadline"
        );
        break;
    }
    let status = process.child.wait().expect("bootstrap child join failed");
    process.joined = true;
    assert_eq!(
        status.code(),
        Some(1),
        "bootstrap accepted an unsafe operator input"
    );

    let mut output = String::new();
    File::open(log)
        .unwrap()
        .take(65_537)
        .read_to_string(&mut output)
        .unwrap();
    assert!(
        output.len() <= 65_536,
        "bootstrap capture exceeded its bound"
    );
    for forbidden in [
        "private-bootstrap-",
        "private-runtime-",
        "http://",
        "https://",
        directory.to_str().unwrap(),
    ] {
        assert!(
            !output.contains(forbidden),
            "bootstrap logs exposed private operator input"
        );
    }
    let events: Vec<Value> = output
        .lines()
        .map(|line| serde_json::from_str(line).expect("bootstrap output was not structured JSON"))
        .collect();
    assert_eq!(events.len(), 1, "rejected startup entered another workflow");
    let failure = &events[0];
    assert_eq!(failure["level"], "ERROR");
    assert_eq!(failure["fields"]["event"], "real_debrid_service_failed");
    assert_eq!(failure["fields"]["outcome"], "failed");
    assert_eq!(failure["fields"]["reason"], reason);
    assert!(failure["fields"]["elapsed_ms"].as_u64().is_some());
    let span = failure["spans"]
        .as_array()
        .expect("bootstrap service span missing")
        .iter()
        .find(|span| span["name"] == "real_debrid_service")
        .expect("bootstrap service correlation missing");
    uuid::Uuid::parse_str(span["service_id"].as_str().unwrap())
        .expect("bootstrap service correlation was not a UUID");
}
