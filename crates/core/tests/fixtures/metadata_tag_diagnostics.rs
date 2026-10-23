#![allow(dead_code)]

use std::{
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

pub struct Tool {
    pub executable: PathBuf,
    directory: PathBuf,
}

pub enum Completion {
    Diagnostic,
    NonzeroExit,
}

struct Process {
    pid: libc::pid_t,
    input: PathBuf,
}

impl Tool {
    pub fn new(directory: &Path, completion: Completion) -> Self {
        let directory = directory.join("diagnostic-protocol");
        std::fs::create_dir(&directory).unwrap();
        let executable = directory.join("owned-ffprobe");
        let script = r#"#!/usr/bin/perl
use strict;
use warnings;
my $root = @@ROOT@@;
if ($ARGV[0] eq '-version') { exec '/usr/bin/ffprobe', @ARGV; exit 93; }
my ($input, $selector);
for my $index (0..$#ARGV) {
  $input = $ARGV[$index + 1] if $ARGV[$index] eq '-i';
  $selector = $ARGV[$index + 1] if $ARGV[$index] eq '-show_entries';
}
sub marker {
  my ($name, $value) = @_;
  open my $file, '>', "$root/$name.pending" or die "fixture marker failed";
  print {$file} $value or die "fixture marker failed";
  close $file or die "fixture marker failed";
  rename "$root/$name.pending", "$root/$name" or die "fixture marker failed";
}
sub await_marker {
  my ($name) = @_;
  select undef, undef, undef, 0.001 until -e "$root/$name";
}
sub write_all {
  my ($handle, $value) = @_;
  my $offset = 0;
  while ($offset < length $value) {
    my $written = syswrite($handle, $value, length($value) - $offset, $offset);
    die "fixture output failed" unless defined $written && $written > 0;
    $offset += $written;
  }
}
unless ($selector =~ /MUSICBRAINZ_ARTISTID/) {
  marker('ordinary', "$$\n$input\n");
  marker('ordinary_ack', -e "$root/final_ack" ? 'acknowledged' : 'unacknowledged');
  exit 91;
}
marker('expanded', "$$\n$input\n");
my $capacity = fcntl(STDOUT, 1032, 0);
$capacity = fcntl(STDOUT, 1031, 65536) if defined $capacity && $capacity < 65536;
unless (defined $capacity && $capacity >= 65536) {
  marker('constraint', 'stdout_pipe_capacity');
  exit 92;
}
marker('ready', "$capacity");
await_marker('prefix_release');
write_all(\*STDOUT, 'P');
while (1) {
  my $queued = pack('i', 0);
  ioctl(STDOUT, 0x541b, $queued) or die "fixture pipe inspection failed";
  last if unpack('i', $queued) == 0;
  select undef, undef, undef, 0.001;
}
marker('prefix_drained', 'drained');
await_marker('final_release');
my $body = 'private-combined-stdout-canary';
write_all(\*STDOUT, $body . ('x' x (65536 - length $body)));
@@COMPLETION@@
"#
        .replace("@@ROOT@@", &quote(&directory))
        .replace(
            "@@COMPLETION@@",
            match completion {
                Completion::Diagnostic => concat!(
                    "write_all(\\*STDERR, 'private-combined-stderr-canary');\n",
                    "marker('final_ack', 'both_writes_completed');\n",
                    "sleep 300;",
                ),
                Completion::NonzeroExit => concat!(
                    "marker('final_ack', 'stdout_write_completed');\n",
                    "exit 91;",
                ),
            },
        );
        std::fs::write(&executable, script).unwrap();
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
        Self {
            executable,
            directory,
        }
    }

    pub async fn ready(&self) {
        self.wait("ready").await;
        let capacity: usize = std::fs::read_to_string(self.directory.join("ready"))
            .unwrap()
            .parse()
            .unwrap();
        assert!(
            capacity >= 65536,
            "The fixture requires one complete frame of pipe capacity"
        );
        let process = self.process("expanded").unwrap();
        assert!(process.running());
        let input = std::fs::metadata(&process.input).unwrap();
        let parent = std::fs::metadata(process.input.parent().unwrap()).unwrap();
        assert_eq!(input.permissions().mode() & 0o777, 0o600);
        assert_eq!(parent.permissions().mode() & 0o777, 0o700);
    }

    pub async fn drain_prefix(&self) {
        std::fs::write(self.directory.join("prefix_release"), []).unwrap();
        self.wait("prefix_drained").await;
    }

    pub fn complete_both_writes_while_runtime_is_paused(&self) {
        self.complete_final_frame("both_writes_completed");
    }

    pub fn complete_stdout_and_observe_exit_while_runtime_is_paused(&self) {
        self.complete_final_frame("stdout_write_completed");
        let process = self.process("expanded").unwrap();
        let started = Instant::now();
        loop {
            let mut status = std::mem::MaybeUninit::<libc::siginfo_t>::zeroed();
            let observed = unsafe {
                libc::waitid(
                    libc::P_PID,
                    process.pid as libc::id_t,
                    status.as_mut_ptr(),
                    libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
                )
            };
            assert_eq!(
                observed, 0,
                "The fixture must observe its owned child without reaping it"
            );
            let status = unsafe { status.assume_init() };
            if unsafe { status.si_pid() } == process.pid {
                assert_eq!(status.si_code, libc::CLD_EXITED);
                assert_eq!(unsafe { status.si_status() }, 91);
                return;
            }
            assert!(
                started.elapsed() < Duration::from_secs(3),
                "The owned tool must exit naturally before the runtime resumes"
            );
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    fn complete_final_frame(&self, expected: &str) {
        std::fs::write(self.directory.join("final_release"), []).unwrap();
        let started = Instant::now();
        while !self.directory.join("final_ack").exists() {
            assert!(
                started.elapsed() < Duration::from_secs(3),
                "The owned tool must acknowledge both successful pipe writes"
            );
            std::thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(
            std::fs::read_to_string(self.directory.join("final_ack")).unwrap(),
            expected
        );
    }

    pub fn ordinary_attempted(&self) -> bool {
        if self.directory.join("ordinary").exists() {
            assert_eq!(
                std::fs::read_to_string(self.directory.join("ordinary_ack")).unwrap(),
                "acknowledged"
            );
            true
        } else {
            false
        }
    }

    pub async fn retired(&self) -> bool {
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if ["expanded", "ordinary"].into_iter().all(|mode| {
                    self.process(mode).is_none_or(|process| {
                        !process.running()
                            && !process.input.exists()
                            && !process.input.parent().unwrap().exists()
                    })
                }) {
                    return;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .is_ok()
    }

    pub fn kill_owned(&self) {
        for mode in ["expanded", "ordinary"] {
            if let Some(process) = self.process(mode)
                && process.running()
            {
                unsafe { libc::kill(process.pid, libc::SIGKILL) };
            }
        }
    }

    async fn wait(&self, marker: &str) {
        tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                assert!(
                    !self.directory.join("constraint").exists(),
                    "Fixture constraint: stdout pipe capacity cannot hold 65536 bytes; the kernel per-user pipe buffer quota is exhausted, so run the suite as an unprivileged user or with CAP_SYS_RESOURCE"
                );
                if self.directory.join(marker).exists() {
                    return;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("The owned tool must reach its protocol marker");
    }

    fn process(&self, mode: &str) -> Option<Process> {
        let text = std::fs::read_to_string(self.directory.join(mode)).ok()?;
        if !text.ends_with('\n') {
            return None;
        }
        let mut lines = text.lines();
        let pid = lines.next()?.parse().ok()?;
        assert!(pid > 1);
        Some(Process {
            pid,
            input: lines.next()?.into(),
        })
    }
}

impl Process {
    fn running(&self) -> bool {
        unsafe { libc::kill(self.pid, 0) == 0 }
    }
}

fn quote(path: &Path) -> String {
    format!(
        "'{}'",
        path.to_string_lossy()
            .replace('\\', "\\\\")
            .replace('\'', "\\'")
    )
}
