use std::{
    ffi::{CStr, c_char, c_int, c_uint, c_void},
    sync::{
        Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
};

use libsqlite3_sys as sqlite;

static ACTIVE: AtomicBool = AtomicBool::new(false);
static STEPS: AtomicU64 = AtomicU64::new(0);
static STATEMENT: Mutex<Option<String>> = Mutex::new(None);
pub const BUDGET: u64 = 10_000;

pub struct Registration;
pub struct Measurement;

impl Registration {
    pub fn new() -> Self {
        assert_eq!(
            unsafe { sqlite::sqlite3_auto_extension(Some(install)) },
            sqlite::SQLITE_OK
        );
        Self
    }
}

impl Drop for Registration {
    fn drop(&mut self) {
        ACTIVE.store(false, Ordering::SeqCst);
        unsafe { sqlite::sqlite3_cancel_auto_extension(Some(install)) };
    }
}

impl Measurement {
    pub fn start() -> Self {
        STEPS.store(0, Ordering::SeqCst);
        *STATEMENT.lock().unwrap() = None;
        ACTIVE.store(true, Ordering::SeqCst);
        Self
    }

    pub fn finish(self) -> (u64, String) {
        ACTIVE.store(false, Ordering::SeqCst);
        (
            STEPS.load(Ordering::SeqCst),
            STATEMENT
                .lock()
                .unwrap()
                .clone()
                .expect("The actual rate decision was not traced"),
        )
    }
}

impl Drop for Measurement {
    fn drop(&mut self) {
        ACTIVE.store(false, Ordering::SeqCst);
    }
}

unsafe extern "C" fn progress(_: *mut c_void) -> c_int {
    if ACTIVE.load(Ordering::SeqCst) && STEPS.fetch_add(100, Ordering::SeqCst) + 100 > BUDGET {
        1
    } else {
        0
    }
}

unsafe extern "C" fn trace(
    _: c_uint,
    _: *mut c_void,
    statement: *mut c_void,
    _: *mut c_void,
) -> c_int {
    if !ACTIVE.load(Ordering::SeqCst) {
        return 0;
    }
    let text = unsafe { sqlite::sqlite3_sql(statement.cast()) };
    if text.is_null() {
        return 0;
    }
    let text = unsafe { CStr::from_ptr(text) }.to_bytes();
    if text.len() <= 2048 {
        let text = String::from_utf8_lossy(text);
        if text.starts_with("SELECT count(*)") && text.contains("created_at") {
            *STATEMENT.lock().unwrap_or_else(|error| error.into_inner()) = Some(text.into_owned());
        }
    }
    0
}

unsafe extern "C" fn install(
    connection: *mut sqlite::sqlite3,
    _: *mut *mut c_char,
    _: *const sqlite::sqlite3_api_routines,
) -> c_int {
    unsafe {
        sqlite::sqlite3_progress_handler(connection, 100, Some(progress), std::ptr::null_mut());
        sqlite::sqlite3_trace_v2(
            connection,
            sqlite::SQLITE_TRACE_STMT as u32,
            Some(trace),
            std::ptr::null_mut(),
        )
    }
}
