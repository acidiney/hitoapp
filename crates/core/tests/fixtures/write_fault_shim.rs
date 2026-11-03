use std::{
    path::{Path, PathBuf},
    time::Duration,
};

const SOURCE: &str = r#"
#define _GNU_SOURCE
#include <dlfcn.h>
#include <errno.h>
#include <fcntl.h>
#include <limits.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/types.h>
#include <sys/uio.h>
#include <unistd.h>

static int flagged(const char *name) {
    const char *directory = getenv("HITO_FAULT_DIR");
    char flag[PATH_MAX];
    if (!directory) return 0;
    snprintf(flag, sizeof flag, "%s/%s", directory, name);
    return access(flag, F_OK) == 0;
}

static int path_of(int fd, char *path) {
    char link[64];
    snprintf(link, sizeof link, "/proc/self/fd/%d", fd);
    ssize_t size = readlink(link, path, PATH_MAX - 1);
    if (size <= 0) return 0;
    path[size] = 0;
    return 1;
}

static int on_full_disk(int fd) {
    char path[PATH_MAX];
    int disk = flagged("full-disk");
    int database = flagged("full-db");
    if (!disk && !database) return 0;
    if (!path_of(fd, path)) return 0;
    if (disk && strstr(path, ".downloads/")) return 1;
    return database && strstr(path, "hito.db") != 0;
}

static int in_downloads_directory(int fd) {
    char path[PATH_MAX];
    size_t size;
    if (!path_of(fd, path)) return 0;
    size = strlen(path);
    return size >= 10 && strcmp(path + size - 10, ".downloads") == 0;
}

ssize_t write(int fd, const void *buffer, size_t count) {
    static ssize_t (*original)(int, const void *, size_t);
    if (!original) original = dlsym(RTLD_NEXT, "write");
    if (on_full_disk(fd)) {
        errno = ENOSPC;
        return -1;
    }
    return original(fd, buffer, count);
}

ssize_t writev(int fd, const struct iovec *vector, int count) {
    static ssize_t (*original)(int, const struct iovec *, int);
    if (!original) original = dlsym(RTLD_NEXT, "writev");
    if (on_full_disk(fd)) {
        errno = ENOSPC;
        return -1;
    }
    return original(fd, vector, count);
}

ssize_t pwrite(int fd, const void *buffer, size_t count, off_t offset) {
    static ssize_t (*original)(int, const void *, size_t, off_t);
    if (!original) original = dlsym(RTLD_NEXT, "pwrite");
    if (on_full_disk(fd)) {
        errno = ENOSPC;
        return -1;
    }
    return original(fd, buffer, count, offset);
}

ssize_t pwrite64(int fd, const void *buffer, size_t count, off_t offset) {
    static ssize_t (*original)(int, const void *, size_t, off_t);
    if (!original) original = dlsym(RTLD_NEXT, "pwrite64");
    if (on_full_disk(fd)) {
        errno = ENOSPC;
        return -1;
    }
    return original(fd, buffer, count, offset);
}

int fsync(int fd) {
    static int (*original)(int);
    if (!original) original = dlsym(RTLD_NEXT, "fsync");
    if (on_full_disk(fd) || (flagged("full-sync-dir") && in_downloads_directory(fd))) {
        errno = ENOSPC;
        return -1;
    }
    return original(fd);
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
    let source = directory.join("disk.c");
    let library = directory.join("disk.so");
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
