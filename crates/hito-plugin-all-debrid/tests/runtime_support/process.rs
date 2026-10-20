use super::inputs::Inputs;
use serde_json::Value;
use std::{
    fs::{File, OpenOptions},
    io::Read,
    net::{SocketAddr, TcpListener},
    os::{
        fd::{AsRawFd, FromRawFd, OwnedFd},
        unix::fs::OpenOptionsExt,
    },
    path::{Path, PathBuf},
    process::{Child, Command, ExitStatus, Stdio},
    time::{Duration, Instant},
};

pub struct Host {
    child: Child,
    notification: OwnedFd,
    exit: Option<ExitStatus>,
    pub address: SocketAddr,
    log: PathBuf,
}

impl Host {
    pub fn start(directory: &Path, inputs: &Inputs) -> Self {
        let reservation = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = reservation.local_addr().unwrap();
        drop(reservation);
        let log = directory.join("owned-private-bootstrap-output.jsonl");
        let output = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&log)
            .unwrap();
        let mut child = Command::new(env!("CARGO_BIN_EXE_hito-plugin-all-debrid"))
            .arg("--provider-token-file")
            .arg(&inputs.provider)
            .arg("--service-token-file")
            .arg(&inputs.service)
            .arg("--receipt-key-file")
            .arg(&inputs.receipt)
            .arg("--catalog-file")
            .arg(&inputs.catalog)
            .arg("--listen")
            .arg(address.to_string())
            .env_clear()
            .env("RUST_LOG", "trace")
            .stdin(Stdio::null())
            .stdout(Stdio::from(output.try_clone().unwrap()))
            .stderr(Stdio::from(output))
            .spawn()
            .unwrap();
        let descriptor = unsafe { libc::syscall(libc::SYS_pidfd_open, child.id(), 0) };
        if descriptor < 0 {
            let _ = child.kill();
            let _ = child.wait();
            panic!("the owned Linux child must provide process notification");
        }
        let notification = unsafe { OwnedFd::from_raw_fd(descriptor.try_into().unwrap()) };
        Self {
            child,
            notification,
            exit: None,
            address,
            log,
        }
    }

    pub fn wait_ready(&mut self) {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if self
                .logs()
                .lines()
                .filter_map(|line| serde_json::from_str::<Value>(line).ok())
                .any(|event| event["fields"]["event"] == "all_debrid_service_ready")
            {
                return;
            }
            self.exit = self.child.try_wait().unwrap();
            assert!(
                self.exit.is_none(),
                "the owned runtime exited before its readiness acknowledgement"
            );
            assert!(
                Instant::now() < deadline,
                "the owned runtime did not acknowledge readiness in time"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    pub fn signal_term(&self) {
        let result = unsafe {
            libc::syscall(
                libc::SYS_pidfd_send_signal,
                self.notification.as_raw_fd(),
                libc::SIGTERM,
                std::ptr::null::<libc::siginfo_t>(),
                0,
            )
        };
        assert_eq!(result, 0, "the owned child must receive SIGTERM");
    }

    pub fn wait_exit(&mut self) -> ExitStatus {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            assert!(
                !remaining.is_zero(),
                "the owned runtime must join its shutdown within its bound"
            );
            let mut event = libc::pollfd {
                fd: self.notification.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            };
            let ready = unsafe { libc::poll(&mut event, 1, remaining.as_millis().max(1) as i32) };
            if ready < 0
                && std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted
            {
                continue;
            }
            assert!(
                ready == 1 && event.revents & libc::POLLIN != 0,
                "the owned child must acknowledge exit"
            );
            let status = self.child.wait().unwrap();
            self.exit = Some(status);
            return status;
        }
    }

    pub fn logs(&self) -> String {
        let mut output = String::new();
        File::open(&self.log)
            .unwrap()
            .take(65_537)
            .read_to_string(&mut output)
            .unwrap();
        assert!(
            output.len() <= 65_536,
            "owned runtime logs must remain bounded"
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
