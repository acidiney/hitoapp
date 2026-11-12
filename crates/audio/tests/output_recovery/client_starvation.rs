use std::{
    fs,
    io::{BufRead, BufReader, Write},
    os::unix::process::CommandExt,
    process::{Child, Command, ExitStatus, Stdio},
    sync::mpsc,
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

const CONTROL: &str = r#"
import os,select,signal,sys,time
from pathlib import Path

fds=[]
result=0

def identity(pid):
    fields=Path(f'/proc/{pid}/stat').read_text().rsplit(') ',1)[1].split()
    return int(fields[1]),fields[19],fields[0]

def fail(*_):
    raise RuntimeError('owned controller interrupted')

try:
    role,owner,wrapper,started,mode=sys.argv[1:6]
    owner,wrapper=int(owner),int(wrapper)
    assert owner==os.getppid() and identity(owner)[:2]==(wrapper,started)
    assert Path(f'/proc/{owner}').stat().st_uid==os.getuid()
    assert Path(f'/proc/{wrapper}/comm').read_text().strip()=='runuser'
    assert os.getsid(owner)==os.getsid(wrapper)==os.getsid(0)
    fds=[os.pidfd_open(owner),os.pidfd_open(wrapper)]
    assert identity(owner)[:2]==(wrapper,started)
    if role=='watchdog':
        peer,peer_started=int(sys.argv[6]),sys.argv[7]
        assert identity(peer)[:2]==(owner,peer_started)
        peer_fd=os.pidfd_open(peer)
        assert identity(peer)[:2]==(owner,peer_started)
        fds.append(peer_fd)
        exited=select.poll();exited.register(peer_fd,select.POLLIN)
    for number in [signal.SIGTERM,signal.SIGINT,signal.SIGALRM]:
        signal.signal(number,fail)
    signal.alarm(3)
    print('ready',flush=True)
    command=sys.stdin.readline()
    if command:
        assert command=='go\n'
        if role=='controller':
            signal.pidfd_send_signal(fds[0],signal.SIGSTOP)
            until=time.monotonic()+1
            while identity(owner)[2]!='T':
                assert time.monotonic()<until
                time.sleep(.001)
            if mode=='kill':
                os.kill(os.getpid(),signal.SIGKILL)
            time.sleep(.4)
        else:
            until=time.monotonic()+1
            while not exited.poll(0):
                if time.monotonic()>=until:
                    signal.pidfd_send_signal(peer_fd,signal.SIGKILL)
                    assert exited.poll(1000)
                    break
                time.sleep(.001)
except BaseException:
    result=1
finally:
    signal.alarm(0)
    for number in [signal.SIGTERM,signal.SIGINT,signal.SIGALRM]:
        signal.signal(number,signal.SIG_IGN)
    for descriptor in fds[:2]:
        try: signal.pidfd_send_signal(descriptor,signal.SIGCONT)
        except OSError: pass
    for descriptor in fds:
        os.close(descriptor)
sys.exit(result)
"#;

struct Process {
    child: Option<Child>,
    reader: Option<JoinHandle<()>>,
}

impl Process {
    fn ready(arguments: &[String]) -> Self {
        let child = Command::new("python3")
            .args(["-c", CONTROL])
            .args(arguments)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .process_group(0)
            .spawn()
            .unwrap();
        let mut owner = Self {
            child: Some(child),
            reader: None,
        };
        let stdout = owner.child.as_mut().unwrap().stdout.take().unwrap();
        let (ready, received) = mpsc::sync_channel(1);
        owner.reader = Some(thread::spawn(move || {
            let mut packet = String::new();
            let valid =
                BufReader::new(stdout).read_line(&mut packet).is_ok() && packet == "ready\n";
            let _ = ready.send(valid);
        }));
        assert!(
            received.recv_timeout(Duration::from_secs(2)).unwrap(),
            "owned client controller failed readiness"
        );
        owner
    }

    fn id(&self) -> u32 {
        self.child.as_ref().unwrap().id()
    }

    fn go(&mut self) {
        let input = self.child.as_mut().unwrap().stdin.as_mut().unwrap();
        input.write_all(b"go\n").unwrap();
        input.flush().unwrap();
    }

    fn join(&mut self) -> std::io::Result<ExitStatus> {
        let mut child = self.child.take().unwrap();
        drop(child.stdin.take());
        let deadline = Instant::now() + Duration::from_secs(3);
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break Ok(status),
                Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(5)),
                _ => {
                    let _ = child.kill();
                    break child.wait();
                }
            }
        };
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
        status
    }
}

impl Drop for Process {
    fn drop(&mut self) {
        if self.child.is_some() {
            let _ = self.join();
        }
    }
}

pub(super) struct Outcome {
    pub controller: ExitStatus,
    pub watchdog: ExitStatus,
}

pub(super) struct ClientStarvation {
    controller: Process,
    watchdog: Process,
}

impl ClientStarvation {
    pub fn ready(kill_controller: bool) -> Self {
        let owner = std::process::id();
        let status = fs::read_to_string("/proc/self/status").unwrap();
        let wrapper: u32 = status
            .lines()
            .find_map(|line| line.strip_prefix("PPid:"))
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        assert!(owner > 1 && wrapper > 1);
        assert_eq!(
            fs::read_to_string(format!("/proc/{wrapper}/comm"))
                .unwrap()
                .trim(),
            "runuser"
        );
        let identity = fs::read_to_string("/proc/self/stat").unwrap();
        let started = identity
            .rsplit_once(") ")
            .unwrap()
            .1
            .split_whitespace()
            .nth(19)
            .unwrap();
        let mode = if kill_controller { "kill" } else { "normal" };
        let mut arguments = vec![
            "controller".into(),
            owner.to_string(),
            wrapper.to_string(),
            started.into(),
            mode.into(),
        ];
        let controller = Process::ready(&arguments);
        let peer_identity = fs::read_to_string(format!("/proc/{}/stat", controller.id())).unwrap();
        let peer_started = peer_identity
            .rsplit_once(") ")
            .unwrap()
            .1
            .split_whitespace()
            .nth(19)
            .unwrap();
        arguments[0] = "watchdog".into();
        arguments.push(controller.id().to_string());
        arguments.push(peer_started.into());
        let watchdog = Process::ready(&arguments);
        Self {
            controller,
            watchdog,
        }
    }

    pub fn ids(&self) -> [u32; 2] {
        [self.controller.id(), self.watchdog.id()]
    }

    pub fn cancel(mut self) -> Outcome {
        let controller = self.controller.join().unwrap();
        let watchdog = self.watchdog.join().unwrap();
        Outcome {
            controller,
            watchdog,
        }
    }

    pub fn starve(mut self) -> Outcome {
        self.watchdog.go();
        self.controller.go();
        let controller = self.controller.join().unwrap();
        let watchdog = self.watchdog.join().unwrap();
        Outcome {
            controller,
            watchdog,
        }
    }
}
