#![cfg(target_os = "linux")]
#[path = "support/binary.rs"]
mod binary;
#[allow(dead_code)]
mod support;

use hito_plugin_folder::Folder;
use hito_plugin_sdk::{ProviderRecording, SearchPage, SourcePage};
use libsqlite3_sys as sqlite;
use std::{
    ffi::{c_char, c_int},
    fs::{self, OpenOptions, TryLockError},
    io::{self, Read, Seek},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{Condvar, Mutex, OnceLock},
    time::{Duration, Instant},
};
use tokio::sync::oneshot;

const TEST_NAME: &str = "cancelled_open_retains_ownership_until_sqlite_establishment_finishes";
const CHILD_ROOT: &str = "HITO_FOLDER_CANCELLATION_CHILD_ROOT";
static GATE: OnceLock<Gate> = OnceLock::new();

struct Gate {
    entered: Mutex<Option<oneshot::Sender<io::Result<PathBuf>>>>,
    released: Mutex<bool>,
    ready: Condvar,
}
impl Gate {
    fn release(&self) {
        *self
            .released
            .lock()
            .unwrap_or_else(|error| error.into_inner()) = true;
        self.ready.notify_all();
    }
}

unsafe extern "C" fn gate_connection(
    _: *mut sqlite::sqlite3,
    _: *mut *mut c_char,
    _: *const sqlite::sqlite3_api_routines,
) -> c_int {
    let Some(gate) = GATE.get() else {
        return sqlite::SQLITE_ERROR;
    };
    let worker = fs::read_link("/proc/thread-self").map(|path| Path::new("/proc").join(path));
    if let Some(entered) = gate
        .entered
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .take()
    {
        let _ = entered.send(worker);
    }
    let mut released = gate
        .released
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    while !*released {
        released = gate
            .ready
            .wait(released)
            .unwrap_or_else(|error| error.into_inner());
    }
    sqlite::SQLITE_OK
}

struct Registration;
impl Drop for Registration {
    fn drop(&mut self) {
        if let Some(gate) = GATE.get() {
            gate.release();
        }
        unsafe {
            sqlite::sqlite3_cancel_auto_extension(Some(gate_connection));
        }
    }
}

struct ChildProcess(Child);
impl Drop for ChildProcess {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn claim_is_held(state: &Path) -> io::Result<bool> {
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(state.join("owner.lock"))?;
    match file.try_lock() {
        Ok(()) => Ok(false),
        Err(TryLockError::WouldBlock) => Ok(true),
        Err(TryLockError::Error(error)) => Err(error),
    }
}

#[derive(Debug, PartialEq, Eq)]
struct Snapshot {
    recording: serde_json::Value,
    source: String,
}
fn snapshot(service: &binary::Service, bytes: &[u8], title: &str) -> Snapshot {
    let page: SearchPage =
        service.post("/v1/search", serde_json::json!({"query":title,"limit":20}));
    page.validate(20).unwrap();
    assert_eq!(page.items.len(), 1);
    assert!(page.next_cursor.is_none());
    let recording = &page.items[0];
    assert_eq!(recording.title, title);
    assert!(
        recording
            .revision
            .as_deref()
            .is_some_and(|revision| !revision.is_empty())
    );
    let detail: ProviderRecording = service.post(
        "/v1/recordings/detail",
        serde_json::json!({"recording_id":recording.id}),
    );
    detail.validate().unwrap();
    let recording = serde_json::to_value(recording).unwrap();
    assert_eq!(serde_json::to_value(detail).unwrap(), recording);
    let sources: SourcePage = service.post(
        "/v1/sources",
        serde_json::json!({"recording_id":page.items[0].id}),
    );
    sources.validate().unwrap();
    assert_eq!(sources.items.len(), 1);
    assert!(sources.items[0].ready);
    let source = &sources.items[0].id;
    let tag = bytes
        .windows(title.len())
        .position(|part| part == title.as_bytes())
        .unwrap();
    assert_eq!(&bytes[36..40], b"data");
    for range in [tag..tag + title.len(), 44..76] {
        assert_eq!(
            service.audio(source, range.start, range.end),
            (206, bytes[range].to_vec())
        );
    }
    Snapshot {
        recording,
        source: source.clone(),
    }
}

fn child_case(root: &Path) {
    let music = root.join("private-music");
    fs::create_dir(&music).unwrap();
    let title = "Retained startup recording";
    let bytes = support::write_wav(&music.join("private-audio.wav"), Some(title), 440);
    let state = root.join("private-state");
    binary::initialize(&music, &state);
    let service = binary::Service::start(&music, &state, &root.join("before.jsonl"));
    let before = snapshot(&service, &bytes, title);
    service.stop();

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .unwrap();
    let (entered, arrived) = oneshot::channel();
    assert!(
        GATE.set(Gate {
            entered: Mutex::new(Some(entered)),
            released: Mutex::new(false),
            ready: Condvar::new(),
        })
        .is_ok()
    );
    let registration = Registration;
    assert_eq!(
        unsafe { sqlite::sqlite3_auto_extension(Some(gate_connection)) },
        sqlite::SQLITE_OK
    );
    let opening_music = music.clone();
    let opening_state = state.clone();
    let (worker, cancelled, held, cleanup) = runtime.block_on(async {
        let opening =
            tokio::spawn(async move { Folder::open(&opening_music, &opening_state).await });
        let worker = tokio::time::timeout(Duration::from_secs(5), arrived).await;
        opening.abort();
        let cancelled = tokio::time::timeout(Duration::from_secs(5), opening).await;
        let held = claim_is_held(&state);
        GATE.get().unwrap().release();
        let cleanup = if let Ok(Ok(Ok(worker))) = &worker {
            tokio::time::timeout(Duration::from_secs(5), async {
                while worker.try_exists()? {
                    tokio::task::yield_now().await;
                }
                while claim_is_held(&state)? {
                    tokio::task::yield_now().await;
                }
                Ok::<_, io::Error>(())
            })
            .await
        } else {
            Ok(Err(io::Error::other(
                "SQLite worker did not report its pending phase",
            )))
        };
        (worker, cancelled, held, cleanup)
    });
    drop(registration);
    runtime.shutdown_timeout(Duration::from_secs(5));

    worker
        .expect("SQLite connection gate was not entered")
        .expect("SQLite connection callback lost its receiver")
        .expect("SQLite worker could not identify its live thread");
    assert!(
        cancelled
            .expect("public open cancellation did not join")
            .err()
            .is_some_and(|error| error.is_cancelled())
    );
    cleanup
        .expect("SQLite worker cleanup did not finish")
        .expect("SQLite cleanup observation failed");
    assert!(
        held.expect("state ownership could not be sampled"),
        "cancelled open released ownership while SQLite establishment was still pending"
    );

    let service = binary::Service::start(&music, &state, &root.join("after.jsonl"));
    assert_eq!(snapshot(&service, &bytes, title), before);
    service.stop();
}

#[test]
fn cancelled_open_retains_ownership_until_sqlite_establishment_finishes() {
    if let Some(root) = std::env::var_os(CHILD_ROOT) {
        child_case(Path::new(&root));
        return;
    }
    let directory = tempfile::tempdir().unwrap();
    let mut output = tempfile::tempfile().unwrap();
    let mut child = ChildProcess(
        Command::new(std::env::current_exe().unwrap())
            .args(["--exact", TEST_NAME, "--nocapture"])
            .env(CHILD_ROOT, directory.path())
            .stdout(Stdio::from(output.try_clone().unwrap()))
            .stderr(Stdio::from(output.try_clone().unwrap()))
            .spawn()
            .unwrap(),
    );
    let deadline = Instant::now() + Duration::from_secs(60);
    let status = loop {
        if let Some(status) = child.0.try_wait().unwrap() {
            break Some(status);
        }
        if Instant::now() >= deadline {
            child.0.kill().unwrap();
            child.0.wait().unwrap();
            break None;
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    output.rewind().unwrap();
    let mut logs = String::new();
    output.take(65536).read_to_string(&mut logs).unwrap();
    assert!(
        status.is_some_and(|status| status.success()),
        "isolated cancellation case failed: {logs}"
    );
}
