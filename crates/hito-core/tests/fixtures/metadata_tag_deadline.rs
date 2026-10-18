use std::{
    os::unix::fs::{MetadataExt, PermissionsExt},
    path::{Path, PathBuf},
    time::Duration,
};

pub struct Tool {
    pub executable: PathBuf,
    markers: PathBuf,
    release: PathBuf,
    attempts: PathBuf,
}

pub struct Process {
    pid: libc::pid_t,
    pub input: PathBuf,
    pub selector: String,
}

impl Tool {
    pub fn new(directory: &Path) -> Self {
        let markers = directory.join("probe-processes");
        std::fs::create_dir(&markers).unwrap();
        let release = directory.join("release-expanded-probe");
        let attempts = directory.join("probe-attempts");
        let executable = directory.join("owned-deadline-ffprobe");
        let script = format!(
            r#"#!/bin/sh
set -eu
if [ "$1" = '-version' ]; then exec /usr/bin/ffprobe "$@"; fi
previous=''
input=''
selector=''
for argument do
  if [ "$previous" = '-i' ]; then input=$argument; fi
  if [ "$previous" = '-show_entries' ]; then selector=$argument; fi
  previous=$argument
done
case "$selector" in
  *MUSICBRAINZ_ARTISTID*) mode=expanded ;;
  *) mode=ordinary ;;
esac
printf '%s\n' "$mode" >> {attempts}
printf '%s\n%s\n%s\n' "$$" "$input" "$selector" > {markers}/"$mode"
if [ "$mode" = expanded ]; then
  while [ ! -e {release} ]; do sleep 0.01; done
  printf 'private-deadline-output-canary'
  printf '%070000d' 0
fi
exec sleep 300
"#,
            attempts = quote(&attempts),
            markers = quote(&markers),
            release = quote(&release),
        );
        std::fs::write(&executable, script).unwrap();
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
        Self {
            executable,
            markers,
            release,
            attempts,
        }
    }

    pub async fn started(&self, mode: &str) -> Process {
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if let Some(process) = self.process(mode) {
                    return process;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("The owned FFprobe attempt must reach its private input")
    }

    pub fn release_expanded(&self) {
        std::fs::write(&self.release, []).unwrap();
    }

    pub fn attempts(&self) -> Vec<String> {
        std::fs::read_to_string(&self.attempts)
            .unwrap_or_default()
            .lines()
            .map(str::to_owned)
            .collect()
    }

    pub async fn retired(&self) -> bool {
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if ["expanded", "ordinary"]
                    .into_iter()
                    .all(|mode| self.process(mode).is_none_or(|process| process.retired()))
                {
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

    fn process(&self, mode: &str) -> Option<Process> {
        let text = std::fs::read_to_string(self.markers.join(mode)).ok()?;
        if !text.ends_with('\n') {
            return None;
        }
        let mut lines = text.lines();
        let pid = lines.next()?.parse().ok()?;
        assert!(pid > 1);
        Some(Process {
            pid,
            input: lines.next()?.into(),
            selector: lines.next()?.to_owned(),
        })
    }
}

impl Process {
    pub fn assert_private_input(&self, expected: &[u8]) {
        assert!(self.running(), "The owned FFprobe attempt must remain held");
        let file = std::fs::symlink_metadata(&self.input).unwrap();
        let directory = std::fs::symlink_metadata(self.input.parent().unwrap()).unwrap();
        let owner = unsafe { libc::geteuid() };
        assert!(file.is_file() && directory.is_dir());
        assert_eq!(file.permissions().mode() & 0o777, 0o600);
        assert_eq!(directory.permissions().mode() & 0o777, 0o700);
        assert_eq!(file.uid(), owner);
        assert_eq!(directory.uid(), owner);
        assert_eq!(file.nlink(), 1);
        assert!(std::fs::read(&self.input).unwrap() == expected);
    }

    fn running(&self) -> bool {
        unsafe { libc::kill(self.pid, 0) == 0 }
    }

    fn retired(&self) -> bool {
        !self.running() && !self.input.exists() && !self.input.parent().unwrap().exists()
    }
}

fn quote(path: &Path) -> String {
    format!("'{}'", path.to_string_lossy().replace('\'', "'\\''"))
}
