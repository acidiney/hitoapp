use std::{
    fs,
    io::{BufRead, BufReader, Write},
    os::unix::{fs::MetadataExt, process::CommandExt},
    process::{Child, Command, ExitStatus, Stdio},
    sync::mpsc,
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

const CONTROL: &str = r#"
import os,select,signal,subprocess,sys,time
from pathlib import Path

fds=[]
result=0
launched=time.monotonic()
DRAINED_USEC=1000

def identity(pid):
    fields=Path(f'/proc/{pid}/stat').read_text().rsplit(') ',1)[1].split()
    return int(fields[1]),fields[19],fields[0]

def fail(*_):
    raise RuntimeError('owned controller interrupted')

def stop(descriptor,pid):
    signal.pidfd_send_signal(descriptor,signal.SIGSTOP)
    while identity(pid)[2]!='T':
        time.sleep(.001)

def output_threads(owner):
    observed={}
    for task in Path(f'/proc/{owner}/task').iterdir():
        try:
            name=(task/'comm').read_text().strip()
            if name in ('cpal_alsa_out','threaded-ml'):
                switches=[line for line in (task/'status').read_text().splitlines() if 'ctxt_switches' in line]
                state=(task/'stat').read_text().rsplit(') ',1)[1].split()[0]
                observed[task.name]=(name,state,switches)
        except (FileNotFoundError,ProcessLookupError):
            pass
    return observed

def settle(owner,outputs):
    previous=None
    while True:
        current=output_threads(owner)
        assert all(sum(name==thread for name,_,_ in current.values())==outputs for thread in ('cpal_alsa_out','threaded-ml'))
        if current==previous and all(state=='S' for _,state,_ in current.values()):
            return
        previous=current
        time.sleep(.001)

def queued_usec(owner):
    listing=subprocess.run(['pactl','list','sink-inputs'],capture_output=True,text=True,check=True).stdout
    owned=[block for block in listing.split('Sink Input #')[1:] if f'application.process.id = "{owner}"' in block]
    assert owned
    latency=lambda block,name: int(block.split(f'{name} Latency: ',1)[1].split(' ',1)[0])
    return max(latency(block,'Buffer') for block in owned),latency(owned[0],'Sink')

def drain(owner):
    rendered=None
    while True:
        buffered,sink=queued_usec(owner)
        if buffered>=DRAINED_USEC:
            rendered=None
        elif rendered is not None and sink>rendered:
            break
        else:
            rendered=sink
    queued_usec(owner)

try:
    role,owner,wrapper,started,mode,outputs,backend,backend_parent,backend_started,stall_ms=sys.argv[1:11]
    owner,wrapper,outputs,backend,backend_parent=int(owner),int(wrapper),int(outputs),int(backend),int(backend_parent)
    assert owner==os.getppid() and identity(owner)[:2]==(wrapper,started)
    assert Path(f'/proc/{owner}').stat().st_uid==os.getuid()
    assert Path(f'/proc/{wrapper}/comm').read_text().strip()=='runuser'
    assert os.getsid(owner)==os.getsid(wrapper)==os.getsid(0)
    assert Path(f'/proc/{backend}/comm').read_text().strip()=='pulseaudio'
    assert Path(f'/proc/{backend_parent}/comm').read_text().strip()=='runuser'
    assert Path(f'/proc/{backend}').stat().st_uid==os.getuid()
    fds=[os.pidfd_open(owner),os.pidfd_open(wrapper),os.pidfd_open(backend),os.pidfd_open(backend_parent)]
    assert identity(owner)[:2]==(wrapper,started)
    assert identity(backend)[:2]==(backend_parent,backend_started)
    if role=='watchdog':
        peer,peer_started=int(sys.argv[11]),sys.argv[12]
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
            stalled=time.monotonic()
            stop(fds[2],backend)
            settle(owner,outputs)
            stop(fds[0],owner)
            if mode=='kill':
                os.kill(os.getpid(),signal.SIGKILL)
            time.sleep(max(0,stalled+int(stall_ms)/1000-time.monotonic()))
            signal.pidfd_send_signal(fds[2],signal.SIGCONT)
            drain(owner)
        else:
            until=time.monotonic()+1
            while not exited.poll(0):
                if time.monotonic()>=until:
                    signal.pidfd_send_signal(peer_fd,signal.SIGKILL)
                    assert exited.poll(1000)
                    break
                time.sleep(.001)
except BaseException as error:
    result=1
    print(f'output fault {sys.argv[1]} failed after {time.monotonic()-launched:.3f}s: {type(error).__name__} {error}',file=sys.stderr,flush=True)
finally:
    signal.alarm(0)
    for number in [signal.SIGTERM,signal.SIGINT,signal.SIGALRM]:
        signal.signal(number,signal.SIG_IGN)
    for descriptor in fds[:4]:
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
            .stderr(Stdio::inherit())
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

pub(super) struct OutputFault {
    controller: Process,
    watchdog: Process,
}

struct Backend {
    pid: u32,
    parent: u32,
    started: String,
}

fn started(process: &str) -> String {
    fs::read_to_string(format!("{process}/stat"))
        .unwrap()
        .rsplit_once(") ")
        .unwrap()
        .1
        .split_whitespace()
        .nth(19)
        .unwrap()
        .to_owned()
}

fn parent(process: &str) -> u32 {
    fs::read_to_string(format!("{process}/status"))
        .unwrap()
        .lines()
        .find_map(|line| line.strip_prefix("PPid:"))
        .unwrap()
        .trim()
        .parse()
        .unwrap()
}

fn private_backend() -> Backend {
    let pid: u32 = std::env::var("HITO_PRIVATE_PULSE_PID")
        .expect("the isolated launcher must expose its owned PulseAudio PID")
        .parse()
        .unwrap();
    assert!(pid > 1 && pid != std::process::id());
    let process = format!("/proc/{pid}");
    let backend_parent = parent(&process);
    let test_parent = parent("/proc/self");
    for pid in [backend_parent, test_parent] {
        assert_eq!(
            fs::read_to_string(format!("/proc/{pid}/comm"))
                .unwrap()
                .trim(),
            "runuser"
        );
    }
    assert_eq!(
        parent(&format!("/proc/{backend_parent}")),
        parent(&format!("/proc/{test_parent}")),
        "the backend and test must share the same isolated launcher"
    );
    assert_eq!(
        fs::metadata(&process).unwrap().uid(),
        fs::metadata("/proc/self").unwrap().uid(),
        "the backend must belong to the fixture account"
    );
    assert_eq!(
        fs::read_to_string(format!("{process}/comm"))
            .unwrap()
            .trim(),
        "pulseaudio"
    );
    let socket = std::env::var("PULSE_SERVER").unwrap();
    let socket = socket.strip_prefix("unix:").unwrap();
    let command = fs::read(format!("{process}/cmdline")).unwrap();
    let expected = format!("--load=module-native-protocol-unix socket={socket} auth-anonymous=1");
    assert!(
        command
            .split(|byte| *byte == 0)
            .any(|argument| argument == expected.as_bytes()),
        "only the private fixture backend may be interrupted"
    );
    Backend {
        started: started(&process),
        pid,
        parent: backend_parent,
    }
}

impl OutputFault {
    pub fn ready(kill_controller: bool, outputs: usize, backend_stall: Duration) -> Self {
        let owner = std::process::id();
        let wrapper = parent("/proc/self");
        assert!(owner > 1 && wrapper > 1);
        assert_eq!(
            fs::read_to_string(format!("/proc/{wrapper}/comm"))
                .unwrap()
                .trim(),
            "runuser"
        );
        let backend = private_backend();
        let mode = if kill_controller { "kill" } else { "normal" };
        let mut arguments = vec![
            "controller".into(),
            owner.to_string(),
            wrapper.to_string(),
            started("/proc/self"),
            mode.into(),
            outputs.to_string(),
            backend.pid.to_string(),
            backend.parent.to_string(),
            backend.started,
            backend_stall.as_millis().to_string(),
        ];
        let controller = Process::ready(&arguments);
        let peer_started = started(&format!("/proc/{}", controller.id()));
        arguments[0] = "watchdog".into();
        arguments.push(controller.id().to_string());
        arguments.push(peer_started);
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

    pub fn interrupt(mut self) -> Outcome {
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
