use super::inputs::Fixture;
use std::{
    fs::{File, OpenOptions},
    io::Read,
    net::TcpListener,
    os::{
        fd::{AsRawFd, FromRawFd, OwnedFd},
        unix::fs::OpenOptionsExt,
    },
    process::{Child, Command, Stdio},
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

pub fn rejected(fixture: &Fixture) -> String {
    let occupied = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = occupied.local_addr().unwrap();
    let log = fixture
        .directory
        .path()
        .join("owned-private-controls-output.jsonl");
    let output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&log)
        .unwrap();
    let child = Command::new(env!("CARGO_BIN_EXE_hito-plugin-all-debrid"))
        .arg("--provider-token-file")
        .arg(&fixture.provider)
        .arg("--service-token-file")
        .arg(&fixture.service)
        .arg("--receipt-key-file")
        .arg(&fixture.receipt)
        .arg("--catalog-file")
        .arg(&fixture.catalog)
        .arg("--listen")
        .arg(address.to_string())
        .env_clear()
        .env("RUST_LOG", "trace")
        .stdin(Stdio::null())
        .stdout(Stdio::from(output.try_clone().unwrap()))
        .stderr(Stdio::from(output))
        .spawn()
        .unwrap();
    let mut process = Process {
        child,
        joined: false,
    };
    let descriptor = unsafe { libc::syscall(libc::SYS_pidfd_open, process.child.id(), 0) };
    assert!(
        descriptor >= 0,
        "the owned Linux child must provide process notification"
    );
    let notification = unsafe { OwnedFd::from_raw_fd(descriptor.try_into().unwrap()) };
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        assert!(
            !remaining.is_zero(),
            "unsafe private input must fail before binding within its deadline"
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
            "the rejected child must retire within its deadline"
        );
        break;
    }
    let status = process.child.wait().unwrap();
    process.joined = true;
    assert_eq!(
        status.code(),
        Some(1),
        "unsafe private input must cause a controlled startup failure"
    );
    assert_eq!(occupied.local_addr().unwrap(), address);
    let mut logs = String::new();
    File::open(log)
        .unwrap()
        .take(65_537)
        .read_to_string(&mut logs)
        .unwrap();
    assert!(
        logs.len() <= 65_536,
        "owned bootstrap TRACE capture must remain bounded"
    );
    logs
}
