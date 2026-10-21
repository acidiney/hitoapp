use super::inputs::{Fixture, PRIVATE};
use std::{
    ffi::OsString,
    io::Read,
    net::TcpListener,
    os::fd::{AsRawFd, FromRawFd, OwnedFd},
    process::{Child, Command, Stdio},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

type Reader = JoinHandle<std::io::Result<Vec<u8>>>;

struct Process {
    child: Child,
    reaped: bool,
    readers: Vec<Reader>,
}

pub fn rejected(fixture: &Fixture, arguments: &[OsString], explicit_listen: bool) -> String {
    let occupied = TcpListener::bind("127.0.0.1:0")
        .unwrap_or_else(|_| panic!("the owned ordering listener must bind"));
    let address = occupied
        .local_addr()
        .unwrap_or_else(|_| panic!("the owned ordering listener must identify"));
    let mut command = Command::new(env!("CARGO_BIN_EXE_hito-plugin-premiumize"));
    if explicit_listen {
        command.arg("--listen").arg(address.to_string());
    }
    let child = command
        .args(arguments)
        .current_dir(fixture.directory.path())
        .env_clear()
        .env("RUST_LOG", "trace")
        .env(
            "OWNED_PRIVATE_PREMIUMIZE_BOOTSTRAP_ENV",
            format!("{PRIVATE}environment-value"),
        )
        .env("PREMIUMIZE_API_KEY", super::inputs::PROVIDER_TOKEN)
        .env("HITO_PLUGIN_SERVICE_TOKEN", super::inputs::SERVICE_TOKEN)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap_or_else(|_| panic!("the owned bootstrap executable must spawn"));
    let mut process = Process {
        child,
        reaped: false,
        readers: Vec::new(),
    };
    process.readers.push(capture(
        process
            .child
            .stdout
            .take()
            .expect("owned stdout must be captured"),
    ));
    process.readers.push(capture(
        process
            .child
            .stderr
            .take()
            .expect("owned stderr must be captured"),
    ));
    let descriptor = unsafe { libc::syscall(libc::SYS_pidfd_open, process.child.id(), 0) };
    assert!(
        descriptor >= 0,
        "the owned Linux child must provide process notification"
    );
    let notification = unsafe {
        OwnedFd::from_raw_fd(
            descriptor
                .try_into()
                .unwrap_or_else(|_| panic!("the owned descriptor must fit")),
        )
    };
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        assert!(
            !remaining.is_zero(),
            "bootstrap rejection must complete within its bound"
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
            "the rejected child must acknowledge exit within its bound"
        );
        break;
    }
    let status = process
        .child
        .wait()
        .unwrap_or_else(|_| panic!("the owned bootstrap child must be reaped"));
    process.reaped = true;
    let joined = process
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
                .expect("the owned bootstrap reader must join");
            assert!(bytes.len() <= 65_536, "bootstrap capture must stay bounded");
            bytes
        })
        .collect::<Vec<_>>();
    let stderr = output.pop().expect("owned stderr must join");
    let stdout = output.pop().expect("owned stdout must join");
    assert!(
        stdout.len() + stderr.len() <= 65_536,
        "the complete bootstrap log must stay bounded"
    );
    assert!(
        stderr.is_empty(),
        "bootstrap diagnostics must stay in safe structured stdout"
    );
    assert!(
        status.code() == Some(1),
        "unsafe input must cause a controlled exit one"
    );
    assert!(
        occupied.local_addr().ok() == Some(address),
        "the owned ordering listener must remain reserved"
    );
    String::from_utf8(stdout).unwrap_or_else(|_| panic!("bootstrap logs must be UTF-8 JSON"))
}

impl Drop for Process {
    fn drop(&mut self) {
        if !self.reaped {
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
