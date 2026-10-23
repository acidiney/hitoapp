use std::{
    os::unix::fs::{MetadataExt, PermissionsExt},
    path::{Path, PathBuf},
    time::Duration,
};

pub struct Tools {
    pub ffprobe: PathBuf,
    pub ffmpeg: PathBuf,
    markers: PathBuf,
    release: PathBuf,
}

pub struct Process {
    pid: libc::pid_t,
    input: Option<PathBuf>,
}

impl Tools {
    pub fn new(directory: &Path) -> Self {
        let markers = directory.join("command-processes");
        std::fs::create_dir(&markers).unwrap();
        let release = directory.join("release-commands");
        let versions = directory.join("probe-versions");
        let ffprobe = executable(
            directory,
            "deadline-ffprobe",
            &format!(
                r#"if [ "$1" = '-version' ]; then
  count=0
  if [ -f {versions} ]; then read -r count < {versions}; fi
  count=$((count + 1))
  printf '%s\n' "$count" > {versions}
  if [ "$count" -gt 2 ] && [ ! -e {release} ]; then
    printf '%s\n' "$$" > {markers}/probe-"$$"
    exec sleep 300
  fi
fi
exec /usr/bin/ffprobe "$@""#,
                versions = quote(&versions),
                release = quote(&release),
                markers = quote(&markers),
            ),
        );
        let ffmpeg = executable(
            directory,
            "deadline-ffmpeg",
            &format!(
                r#"if [ "$1" = '-version' ] || [ -e {release} ]; then
  exec /usr/bin/ffmpeg "$@"
fi
previous=''
input=''
for argument do
  if [ "$previous" = '-i' ]; then input=$argument; fi
  previous=$argument
done
printf '%s\n%s\n' "$$" "$input" > {markers}/decoder-"$$"
exec sleep 300"#,
                release = quote(&release),
                markers = quote(&markers),
            ),
        );
        Self {
            ffprobe,
            ffmpeg,
            markers,
            release,
        }
    }

    pub async fn decoders(&self, count: usize) -> Vec<Process> {
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let processes = self.processes(Some("decoder-"));
                if processes.len() == count {
                    return processes;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("The configured decoders must reach their owned inputs")
    }

    pub fn release(&self) {
        std::fs::write(&self.release, []).unwrap();
    }

    pub async fn retired(&self) -> bool {
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if self.processes(None).iter().all(Process::retired) {
                    return;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .is_ok()
    }

    pub fn kill_recorded(&self) {
        for process in self.processes(None) {
            if process.running() {
                unsafe { libc::kill(process.pid, libc::SIGKILL) };
            }
        }
    }

    fn processes(&self, prefix: Option<&str>) -> Vec<Process> {
        std::fs::read_dir(&self.markers)
            .unwrap()
            .filter_map(|entry| {
                let entry = entry.unwrap();
                if prefix
                    .is_some_and(|prefix| !entry.file_name().to_string_lossy().starts_with(prefix))
                {
                    return None;
                }
                let value = std::fs::read_to_string(entry.path()).unwrap();
                if !value.ends_with('\n') {
                    return None;
                }
                let mut lines = value.lines();
                let pid = lines.next()?.parse::<libc::pid_t>().ok()?;
                assert!(pid > 1);
                Some(Process {
                    pid,
                    input: lines
                        .next()
                        .filter(|line| !line.is_empty())
                        .map(PathBuf::from),
                })
            })
            .collect()
    }
}

impl Process {
    pub fn assert_private_input(&self) {
        self.assert_live_input();
        let input = self.input.as_ref().unwrap();
        let metadata = std::fs::symlink_metadata(input).unwrap();
        let directory = std::fs::symlink_metadata(input.parent().unwrap()).unwrap();
        let owner = unsafe { libc::geteuid() };
        assert_eq!(metadata.permissions().mode() & 0o777, 0o600);
        assert_eq!(metadata.uid(), owner);
        assert_eq!(metadata.nlink(), 1);
        assert!(directory.is_dir());
        assert_eq!(directory.uid(), owner);
    }

    pub fn assert_live_input(&self) {
        assert!(
            self.running(),
            "The decoder must still occupy its command slot"
        );
        let input = self.input.as_ref().expect("The decoder must own an input");
        let metadata = std::fs::symlink_metadata(input).unwrap();
        assert!(metadata.is_file());
        assert_eq!(
            std::fs::metadata(input.parent().unwrap())
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
    }

    fn running(&self) -> bool {
        unsafe { libc::kill(self.pid, 0) == 0 }
    }

    fn retired(&self) -> bool {
        !self.running()
            && self
                .input
                .as_ref()
                .is_none_or(|input| !input.exists() && !input.parent().unwrap().exists())
    }
}

fn executable(directory: &Path, name: &str, body: &str) -> PathBuf {
    let path = directory.join(name);
    std::fs::write(&path, format!("#!/bin/sh\nset -eu\n{body}\n")).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
    path
}

fn quote(path: &Path) -> String {
    format!("'{}'", path.to_string_lossy().replace('\'', "'\\''"))
}
