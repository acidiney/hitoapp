#![cfg(target_os = "linux")]

use std::{
    ffi::{CStr, c_char, c_int, c_void},
    fs::{OpenOptions, TryLockError},
    io,
    path::{Path, PathBuf},
    sync::{
        Condvar, Mutex, OnceLock,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use hito_community::{Config, Service};
use libsqlite3_sys as sqlite;
use tokio::sync::oneshot;

static GATE: OnceLock<CloseGate> = OnceLock::new();

struct CloseGate {
    entered: Mutex<Option<oneshot::Sender<io::Result<PathBuf>>>>,
    rejected: AtomicBool,
    callback_exited: AtomicBool,
    released: Mutex<bool>,
    ready: Condvar,
}

impl CloseGate {
    fn release(&self) {
        *self
            .released
            .lock()
            .unwrap_or_else(|error| error.into_inner()) = true;
        self.ready.notify_all();
    }
}

unsafe extern "C" fn reject_journal_mode(
    _: *mut c_void,
    action: c_int,
    name: *const c_char,
    _: *const c_char,
    _: *const c_char,
    _: *const c_char,
) -> c_int {
    if action == sqlite::SQLITE_PRAGMA
        && !name.is_null()
        // SQLite owns the terminated name throughout this authorizer callback.
        && unsafe { CStr::from_ptr(name) }.to_bytes().eq_ignore_ascii_case(b"journal_mode")
    {
        if let Some(gate) = GATE.get() {
            gate.rejected.store(true, Ordering::Release);
        }
        sqlite::SQLITE_DENY
    } else {
        sqlite::SQLITE_OK
    }
}

unsafe extern "C" fn unused_function(
    context: *mut sqlite::sqlite3_context,
    _: c_int,
    _: *mut *mut sqlite::sqlite3_value,
) {
    // SQLite supplies the live context if this unused fixture function is called.
    unsafe { sqlite::sqlite3_result_null(context) };
}

unsafe extern "C" fn hold_close(_: *mut c_void) {
    let Some(gate) = GATE.get() else {
        return;
    };
    let callback_thread =
        std::fs::read_link("/proc/thread-self").map(|path| Path::new("/proc").join(path));
    let first_callback = if let Some(entered) = gate
        .entered
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .take()
    {
        let _ = entered.send(callback_thread);
        true
    } else {
        false
    };
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
    drop(released);
    if first_callback {
        gate.callback_exited.store(true, Ordering::Release);
    }
}

unsafe extern "C" fn install_failure(
    connection: *mut sqlite::sqlite3,
    _: *mut *mut c_char,
    _: *const sqlite::sqlite3_api_routines,
) -> c_int {
    // The auto-extension exclusively receives the actual newly opened handle.
    let status = unsafe {
        sqlite::sqlite3_set_authorizer(connection, Some(reject_journal_mode), std::ptr::null_mut())
    };
    if status != sqlite::SQLITE_OK {
        return status;
    }
    unsafe {
        sqlite::sqlite3_create_function_v2(
            connection,
            c"hito_community_test_close_gate".as_ptr(),
            0,
            sqlite::SQLITE_UTF8 | sqlite::SQLITE_DIRECTONLY,
            std::ptr::null_mut(),
            Some(unused_function),
            None,
            None,
            Some(hold_close),
        )
    }
}

struct Registration;

impl Drop for Registration {
    fn drop(&mut self) {
        if let Some(gate) = GATE.get() {
            gate.release();
        }
        unsafe {
            // This integration target has one case; its static callbacks outlive every worker.
            sqlite::sqlite3_cancel_auto_extension(Some(install_failure));
        }
    }
}

fn claim_is_held(directory: &Path) -> io::Result<bool> {
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(directory.join("owner.lock"))?;
    match file.try_lock() {
        Ok(()) => Ok(false),
        Err(TryLockError::WouldBlock) => Ok(true),
        Err(TryLockError::Error(error)) => Err(error),
    }
}

fn config(directory: &Path) -> Config {
    Config {
        state_directory: directory.to_owned(),
        admin_token: "17a0427d104d87ad8adce74f1fbbe334cb4969d697d0fe4d00bc58e15863a5ad".to_owned(),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn failed_startup_keeps_ownership_until_the_initialized_sqlite_worker_finishes_closing() {
    let fixture = tempfile::tempdir().unwrap();
    let directory = fixture.path().join("community");
    let initial = Service::open(config(&directory)).await.unwrap();
    initial.close().await;
    drop(initial);

    let (entered, arrived) = oneshot::channel();
    assert!(
        GATE.set(CloseGate {
            entered: Mutex::new(Some(entered)),
            rejected: AtomicBool::new(false),
            callback_exited: AtomicBool::new(false),
            released: Mutex::new(false),
            ready: Condvar::new(),
        })
        .is_ok()
    );
    let registration = Registration;
    assert_eq!(
        unsafe { sqlite::sqlite3_auto_extension(Some(install_failure)) },
        sqlite::SQLITE_OK
    );
    let opening_config = config(&directory);
    let mut opening = tokio::spawn(async move { Service::open(opening_config).await });
    let callback_thread = tokio::time::timeout(Duration::from_secs(5), arrived)
        .await
        .expect("SQLite close callback was not entered")
        .expect("SQLite close callback lost its receiver")
        .expect("SQLite close callback could not identify its live thread");
    let returned = tokio::time::timeout(Duration::from_secs(5), &mut opening).await;
    let returned_while_closing = returned.is_ok();
    let held_while_closing = claim_is_held(&directory);
    let callback_thread_was_live = callback_thread.try_exists();
    GATE.get().unwrap().release();
    let completed = match returned {
        Ok(completed) => completed,
        Err(_) => tokio::time::timeout(Duration::from_secs(5), opening)
            .await
            .expect("Startup did not finish after the close gate was released"),
    }
    .expect("Startup task did not join");
    assert!(
        completed.is_err(),
        "Startup accepted a rejected initial PRAGMA"
    );
    assert!(
        GATE.get().unwrap().callback_exited.load(Ordering::Acquire),
        "Startup returned before its SQLite close callback exited"
    );
    assert!(
        !claim_is_held(&directory).unwrap(),
        "Startup returned before releasing state ownership"
    );
    drop(registration);

    assert!(
        GATE.get().unwrap().rejected.load(Ordering::Acquire),
        "The fixture did not reject initial journal-mode setup"
    );
    assert!(
        callback_thread_was_live.unwrap(),
        "The observed SQLite callback thread was not alive at its close barrier"
    );
    let held_while_closing = held_while_closing.unwrap();
    assert!(
        !returned_while_closing && held_while_closing,
        "Startup must retain ownership and await actual SQLite teardown: returned_while_closing={returned_while_closing}, claim_held={held_while_closing}"
    );

    let reopened = Service::open(config(&directory)).await.unwrap();
    reopened.close().await;
}
