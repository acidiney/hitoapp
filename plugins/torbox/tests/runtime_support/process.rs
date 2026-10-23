use super::inputs::Inputs;
use std::{
    ffi::OsString,
    io::Read,
    net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream},
    os::fd::{AsRawFd, FromRawFd, OwnedFd},
    path::Path,
    process::{Child, Command, ExitStatus, Stdio},
    sync::{
        Mutex,
        atomic::{AtomicU8, Ordering},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

type Reader = JoinHandle<std::io::Result<Vec<u8>>>;

pub struct Host {
    child: Child,
    notification: OwnedFd,
    exit: Option<ExitStatus>,
    readers: Vec<Reader>,
    pub address: SocketAddr,
}

pub struct Stopped {
    pub status: ExitStatus,
    pub signalled: bool,
    pub stdout: String,
    pub stderr: Vec<u8>,
}

static NEXT_LOOPBACK_HOST: AtomicU8 = AtomicU8::new(0);
static LISTENER_FORK_EXCLUSION: Mutex<()> = Mutex::new(());

pub fn free_address() -> SocketAddr {
    let _exclusive = LISTENER_FORK_EXCLUSION
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let process = std::process::id();
    let host = NEXT_LOOPBACK_HOST.fetch_add(1, Ordering::Relaxed) % 254 + 1;
    TcpListener::bind((
        Ipv4Addr::new(127, (process >> 8) as u8, process as u8, host),
        0,
    ))
    .and_then(|reservation| reservation.local_addr())
        .unwrap_or_else(|_| panic!("an owned listen address must reserve"))
}

pub fn refuses_connections(address: SocketAddr) -> bool {
    TcpStream::connect(address).is_err()
}

impl Host {
    pub fn start(directory: &Path, inputs: &Inputs, mode: &str) -> Self {
        Self::start_with_arguments(directory, &inputs.arguments(mode), free_address())
    }

    pub fn start_with_arguments(
        directory: &Path,
        arguments: &[OsString],
        address: SocketAddr,
    ) -> Self {
        let spawning = LISTENER_FORK_EXCLUSION
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut child = Command::new(env!("CARGO_BIN_EXE_hito-plugin-torbox"))
            .arg("--listen")
            .arg(address.to_string())
            .args(arguments)
            .current_dir(directory)
            .env_clear()
            .env("RUST_LOG", "trace")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap_or_else(|_| panic!("the owned TorBox executable must spawn"));
        drop(spawning);
        let descriptor = unsafe { libc::syscall(libc::SYS_pidfd_open, child.id(), 0) };
        if descriptor < 0 {
            let _ = child.kill();
            let _ = child.wait();
            panic!("the owned Linux child must provide process notification");
        }
        let notification = unsafe {
            OwnedFd::from_raw_fd(
                descriptor
                    .try_into()
                    .unwrap_or_else(|_| panic!("the owned process descriptor must fit")),
            )
        };
        let mut host = Self {
            child,
            notification,
            exit: None,
            readers: Vec::new(),
            address,
        };
        host.readers.push(capture(
            host.child
                .stdout
                .take()
                .expect("owned stdout must be captured"),
        ));
        host.readers.push(capture(
            host.child
                .stderr
                .take()
                .expect("owned stderr must be captured"),
        ));
        host
    }

    pub fn wait_ready(&mut self) {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            self.exit = self
                .child
                .try_wait()
                .unwrap_or_else(|_| panic!("the owned runtime must be observable"));
            if let Some(status) = self.exit {
                panic!(
                    "the owned TorBox executable exited before readiness, code {:?}",
                    status.code()
                );
            }
            if TcpStream::connect_timeout(&self.address, Duration::from_millis(50)).is_ok() {
                return;
            }
            assert!(
                Instant::now() < deadline,
                "the owned runtime did not become ready in time"
            );
            thread::sleep(Duration::from_millis(10));
        }
    }

    pub fn rejected_and_join(mut self) -> Stopped {
        self.wait_exit();
        self.finish(false)
    }

    pub fn terminate_and_join(mut self) -> Stopped {
        if self.exit.is_none() {
            self.exit = self
                .child
                .try_wait()
                .unwrap_or_else(|_| panic!("the owned runtime exit must be observable"));
        }
        let signalled = self.exit.is_none();
        if signalled {
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
            self.wait_exit();
        }
        self.finish(signalled)
    }

    fn finish(mut self, signalled: bool) -> Stopped {
        let joined = self
            .readers
            .drain(..)
            .map(|reader| reader.join())
            .collect::<Vec<_>>();
        let mut output = joined
            .into_iter()
            .map(|result| {
                let bytes = result
                    .ok()
                    .and_then(Result::ok)
                    .expect("the owned runtime log reader must join");
                assert!(
                    bytes.len() <= 65_536,
                    "the owned runtime log capture must stay bounded"
                );
                bytes
            })
            .collect::<Vec<_>>();
        let stderr = output.pop().expect("owned stderr must join");
        let stdout = output.pop().expect("owned stdout must join");
        assert!(
            stdout.len() + stderr.len() <= 65_536,
            "the complete runtime log must stay bounded"
        );
        Stopped {
            status: self.exit.expect("the owned runtime must be reaped"),
            signalled,
            stdout: String::from_utf8(stdout)
                .unwrap_or_else(|_| panic!("runtime logs must be UTF-8 JSON")),
            stderr,
        }
    }

    fn wait_exit(&mut self) {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            assert!(
                !remaining.is_zero(),
                "the owned runtime must join shutdown within its bound"
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
            self.exit = Some(
                self.child
                    .wait()
                    .unwrap_or_else(|_| panic!("the owned runtime must be reaped")),
            );
            return;
        }
    }
}

impl Drop for Host {
    fn drop(&mut self) {
        if self.exit.is_none() {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
        for reader in self.readers.drain(..) {
            let _ = reader.join();
        }
    }
}

fn capture(reader: impl Read + Send + 'static) -> Reader {
    thread::spawn(move || {
        let mut bytes = Vec::new();
        reader.take(65_537).read_to_end(&mut bytes)?;
        Ok(bytes)
    })
}
