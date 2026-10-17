#![cfg(target_os = "linux")]

#[path = "fixtures/provider_audio.rs"]
mod audio_fixture;
#[path = "fixtures/content_service.rs"]
mod fixture;
mod support;

use audio_fixture::AudioLibrary;
use hito_core::{DownloadRequest, Error};
use std::{
    ffi::CString,
    fs::File,
    future::pending,
    io::Read,
    os::{fd::FromRawFd, unix::ffi::OsStrExt},
    path::Path,
    time::{Duration, Instant},
};
use uuid::Uuid;

#[tokio::test]
async fn removing_an_intent_before_audio_headers_prevents_a_late_owned_writer() {
    let fixture = AudioLibrary::start().await;
    fixture.ready().await;
    let headers = fixture.server.hold("v1/audio").await;
    let body = fixture.server.hold_broken_tail(64).await;
    let owned = fixture.directory.path().join("hito.downloads");
    assert_eq!(owned.parent(), Some(fixture.directory.path()));
    let mut created = CreatedFiles::watch(&owned);
    let accepted = fixture
        .hito
        .downloads
        .create(
            &fixture.actor,
            &fixture.source.recording_id,
            DownloadRequest {
                request_id: Uuid::new_v4(),
            },
        )
        .await
        .unwrap();
    let downloads = fixture.hito.downloads.clone();
    let started = Instant::now();
    let worker = tokio::spawn(async move { downloads.run_next_job_until(pending()).await });
    tokio::time::timeout(Duration::from_secs(2), headers.entered())
        .await
        .unwrap();
    assert_eq!(owned_file_count(&owned), 0);
    fixture
        .hito
        .downloads
        .remove(&fixture.actor, &accepted.intent_id)
        .await
        .unwrap();
    assert_eq!(owned_file_count(&owned), 0);
    assert_eq!(created.drain(), 0);
    assert!(
        started.elapsed() < Duration::from_millis(200),
        "the held-header removal must finish inside the cached authority window"
    );
    headers.open();
    assert!(
        tokio::time::timeout(Duration::from_secs(2), worker)
            .await
            .unwrap()
            .unwrap()
            .unwrap()
    );
    body.open();

    assert_eq!(
        created.drain(),
        0,
        "an owned file was created after its intent removal committed"
    );
    assert_eq!(owned_file_count(&owned), 0);
    assert!(matches!(
        fixture
            .hito
            .downloads
            .status(&fixture.actor, &fixture.source.recording_id)
            .await,
        Err(Error::NotFound)
    ));
    assert_eq!(
        fixture
            .hito
            .catalog
            .sources(&fixture.actor, &fixture.source.recording_id)
            .await
            .unwrap()
            .len(),
        1
    );
    assert_eq!(fixture.server.requests("v1/audio").await.len(), 1);
    assert_eq!(
        std::fs::read(fixture.music.join("original.wav")).unwrap(),
        fixture.original
    );
}

fn owned_file_count(directory: &Path) -> usize {
    let entries: Vec<_> = std::fs::read_dir(directory)
        .unwrap()
        .map(|entry| entry.unwrap())
        .collect();
    for entry in &entries {
        assert!(entry.file_type().unwrap().is_file());
        assert_generated_name(entry.file_name().to_str().unwrap());
    }
    entries.len()
}

fn assert_generated_name(name: &str) {
    let parts: Vec<_> = name.split('.').collect();
    assert_eq!(parts.len(), 3);
    assert!(matches!(parts[2], "part" | "media"));
    for part in &parts[..2] {
        assert_eq!(Uuid::parse_str(part).unwrap().to_string(), *part);
    }
}

struct CreatedFiles(File);

impl CreatedFiles {
    fn watch(directory: &Path) -> Self {
        let directory = CString::new(directory.as_os_str().as_bytes()).unwrap();
        let descriptor = unsafe { libc::inotify_init1(libc::IN_NONBLOCK | libc::IN_CLOEXEC) };
        assert!(descriptor >= 0);
        let file = unsafe { File::from_raw_fd(descriptor) };
        let watch = unsafe {
            libc::inotify_add_watch(
                descriptor,
                directory.as_ptr(),
                libc::IN_CREATE | libc::IN_MOVED_TO,
            )
        };
        assert!(watch >= 0);
        Self(file)
    }

    fn drain(&mut self) -> usize {
        let mut count = 0;
        let mut bytes = [0u8; 4096];
        loop {
            let read = match self.0.read(&mut bytes) {
                Ok(0) => return count,
                Ok(read) => read,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => return count,
                Err(_) => panic!("owned file notifications could not be read"),
            };
            let mut offset = 0;
            while offset < read {
                let header = std::mem::size_of::<libc::inotify_event>();
                assert!(offset + header <= read);
                let event = unsafe {
                    std::ptr::read_unaligned(
                        bytes.as_ptr().add(offset).cast::<libc::inotify_event>(),
                    )
                };
                let end = offset + header + event.len as usize;
                assert!(end <= read);
                assert_eq!(event.mask & libc::IN_Q_OVERFLOW, 0);
                let name = &bytes[offset + header..end];
                let length = name.iter().position(|byte| *byte == 0).unwrap();
                assert_generated_name(std::str::from_utf8(&name[..length]).unwrap());
                count += 1;
                offset = end;
            }
        }
    }
}
