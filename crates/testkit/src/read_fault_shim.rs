use std::{
    path::{Path, PathBuf},
    time::Duration,
};

const SOURCE: &str = r#"
#define _GNU_SOURCE
#include <dlfcn.h>
#include <errno.h>
#include <limits.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include <unistd.h>

static int flagged(const char *name) {
    const char *directory = getenv("HITO_FAULT_DIR");
    char flag[PATH_MAX];
    if (!directory) return 0;
    snprintf(flag, sizeof flag, "%s/%s", directory, name);
    return access(flag, F_OK) == 0;
}

static void wait_while(const char *name) {
    struct timespec pause = {0, 20000000};
    while (flagged(name)) nanosleep(&pause, 0);
}

static int on_faulty_drive(int fd) {
    char link[64], path[PATH_MAX];
    snprintf(link, sizeof link, "/proc/self/fd/%d", fd);
    ssize_t size = readlink(link, path, sizeof path - 1);
    if (size <= 0) return 0;
    path[size] = 0;
    return strstr(path, "hito-faulty") != 0;
}

ssize_t read(int fd, void *buffer, size_t count) {
    static ssize_t (*original)(int, void *, size_t);
    if (!original) original = dlsym(RTLD_NEXT, "read");
    if ((flagged("stall-read") || flagged("fail-read")) && on_faulty_drive(fd)) {
        wait_while("stall-read");
        if (flagged("fail-read")) {
            errno = EIO;
            return -1;
        }
    }
    return original(fd, buffer, count);
}

char *realpath(const char *path, char *resolved) {
    static char *(*original)(const char *, char *);
    if (!original) original = dlsym(RTLD_NEXT, "realpath");
    if (strstr(path, "hito-faulty")) wait_while("stall-open");
    return original(path, resolved);
}
"#;

pub struct Faults {
    directory: PathBuf,
}

impl Faults {
    pub fn set(&self, name: &str) {
        std::fs::write(self.directory.join(name), "").unwrap();
    }

    pub fn clear(&self, name: &str) {
        std::fs::remove_file(self.directory.join(name)).unwrap();
    }
}

pub fn child_faults() -> Option<Faults> {
    std::env::var_os("HITO_FAULT_DIR").map(|directory| Faults {
        directory: directory.into(),
    })
}

pub async fn run_in_child(test: &str) {
    let fixture = tempfile::tempdir().unwrap();
    let faults = fixture.path().join("faults");
    std::fs::create_dir(&faults).unwrap();
    let library = compile(fixture.path());
    let mut child = tokio::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", test, "--nocapture"])
        .env("HITO_FAULT_DIR", &faults)
        .env("LD_PRELOAD", library)
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let status = tokio::time::timeout(Duration::from_secs(240), child.wait())
        .await
        .expect("The fault-injected child did not finish")
        .unwrap();
    assert!(status.success());
}

fn compile(directory: &Path) -> PathBuf {
    let source = directory.join("filesystem.c");
    let library = directory.join("filesystem.so");
    std::fs::write(&source, SOURCE).unwrap();
    assert!(
        std::process::Command::new("cc")
            .args(["-shared", "-fPIC", "-o"])
            .arg(&library)
            .arg(&source)
            .arg("-ldl")
            .status()
            .unwrap()
            .success()
    );
    library
}
