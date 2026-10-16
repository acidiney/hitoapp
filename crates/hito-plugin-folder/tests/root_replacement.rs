#![cfg(target_os = "linux")]
mod support;
use hito_plugin_folder::Folder;
use std::{
    ffi::CString,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread::JoinHandle,
};

struct Exchange {
    stopped: Arc<AtomicBool>,
    task: Option<JoinHandle<()>>,
}
impl Drop for Exchange {
    fn drop(&mut self) {
        self.stopped.store(true, Ordering::Relaxed);
        self.task.take().unwrap().join().unwrap();
    }
}
#[tokio::test]
async fn replacing_the_selected_root_with_a_symlink_never_reads_the_outside_collection() {
    let directory = tempfile::tempdir().unwrap();
    let selected = directory.path().join("selected");
    let outside = directory.path().join("outside");
    let exchange = directory.path().join("exchange");
    std::fs::create_dir(&selected).unwrap();
    std::fs::create_dir(&outside).unwrap();
    support::write_wav(&selected.join("owned.wav"), Some("Owned audio"), 440);
    support::write_wav(
        &outside.join("private.wav"),
        Some("Outside private recording"),
        441,
    );
    std::os::unix::fs::symlink(&outside, &exchange).unwrap();
    let from = CString::new(selected.as_os_str().as_encoded_bytes()).unwrap();
    let to = CString::new(exchange.as_os_str().as_encoded_bytes()).unwrap();
    let stopped = Arc::new(AtomicBool::new(false));
    let signal = stopped.clone();
    let task = std::thread::spawn(move || {
        while !signal.load(Ordering::Relaxed) {
            assert_eq!(
                unsafe {
                    libc::renameat2(
                        libc::AT_FDCWD,
                        from.as_ptr(),
                        libc::AT_FDCWD,
                        to.as_ptr(),
                        libc::RENAME_EXCHANGE,
                    )
                },
                0
            );
            std::thread::yield_now();
        }
    });
    let _exchange = Exchange {
        stopped,
        task: Some(task),
    };
    for _ in 0..500 {
        if let Ok(folder) = Folder::scan(&selected) {
            let result =
                support::search(&folder.router(), "Outside private recording", None, 20).await;
            assert!(
                result.items.is_empty(),
                "root acquisition followed a replacement symlink"
            );
        }
    }
}
