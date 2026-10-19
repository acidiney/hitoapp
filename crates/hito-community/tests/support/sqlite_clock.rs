use std::{
    ffi::{c_char, c_int},
    sync::atomic::{AtomicI64, AtomicU64, Ordering},
};

use libsqlite3_sys as sqlite;

static NOW: AtomicI64 = AtomicI64::new(0);
static READS: AtomicU64 = AtomicU64::new(0);

pub struct Clock;

impl Clock {
    pub fn new(now: i64) -> Self {
        NOW.store(now, Ordering::SeqCst);
        READS.store(0, Ordering::SeqCst);
        assert_eq!(
            unsafe { sqlite::sqlite3_auto_extension(Some(install_clock)) },
            sqlite::SQLITE_OK
        );
        Self
    }

    pub fn set(&self, now: i64) {
        NOW.store(now, Ordering::SeqCst);
    }

    pub fn reads(&self) -> u64 {
        READS.load(Ordering::SeqCst)
    }
}

impl Drop for Clock {
    fn drop(&mut self) {
        unsafe { sqlite::sqlite3_cancel_auto_extension(Some(install_clock)) };
    }
}

unsafe extern "C" fn now(
    context: *mut sqlite::sqlite3_context,
    _: c_int,
    _: *mut *mut sqlite::sqlite3_value,
) {
    READS.fetch_add(1, Ordering::SeqCst);
    unsafe { sqlite::sqlite3_result_int64(context, NOW.load(Ordering::SeqCst)) };
}

unsafe extern "C" fn install_clock(
    connection: *mut sqlite::sqlite3,
    _: *mut *mut c_char,
    _: *const sqlite::sqlite3_api_routines,
) -> c_int {
    unsafe {
        sqlite::sqlite3_create_function_v2(
            connection,
            c"unixepoch".as_ptr(),
            0,
            sqlite::SQLITE_UTF8 | sqlite::SQLITE_INNOCUOUS,
            std::ptr::null_mut(),
            Some(now),
            None,
            None,
            None,
        )
    }
}
