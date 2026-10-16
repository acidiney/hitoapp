#![cfg(target_os = "linux")]
mod support;

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use hito_plugin_folder::Folder;
use hito_plugin_sdk::SourcePage;
use http_body_util::BodyExt;
use std::{
    fs::{self, File, OpenOptions},
    os::{fd::AsRawFd, unix::fs::MetadataExt},
    ptr::NonNull,
    time::SystemTime,
};
use tower::ServiceExt;

struct Mapping {
    address: NonNull<u8>,
    length: usize,
}
impl Mapping {
    fn new(file: &File, length: usize) -> Self {
        let address = unsafe {
            libc::mmap(
                std::ptr::null_mut(),
                length,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_SHARED,
                file.as_raw_fd(),
                0,
            )
        };
        assert_ne!(address, libc::MAP_FAILED);
        Self {
            address: NonNull::new(address.cast()).unwrap(),
            length,
        }
    }
    fn write(&mut self, first: usize, bytes: &[u8]) {
        assert!(first <= self.length && bytes.len() <= self.length - first);
        for (offset, byte) in bytes.iter().enumerate() {
            unsafe {
                self.address
                    .as_ptr()
                    .add(first + offset)
                    .write_volatile(*byte);
            }
        }
    }
}
impl Drop for Mapping {
    fn drop(&mut self) {
        assert_eq!(
            unsafe { libc::munmap(self.address.as_ptr().cast(), self.length) },
            0
        );
    }
}

fn file_stamp(file: &File) -> (u64, u64, u64, SystemTime, i64, i64) {
    let metadata = file.metadata().unwrap();
    (
        metadata.dev(),
        metadata.ino(),
        metadata.len(),
        metadata.modified().unwrap(),
        metadata.ctime(),
        metadata.ctime_nsec(),
    )
}

#[tokio::test]
async fn a_retained_audio_editor_mapping_cannot_change_the_indexed_playback_representation() {
    let directory = tempfile::tempdir().unwrap();
    let music = directory.path().join("private-root");
    fs::create_dir(&music).unwrap();
    let selected = music.join("private-editor.wav");
    let title = "Retained editor audio";
    let original = support::write_wav(&selected, Some(title), 440);
    assert_eq!(&original[36..40], b"data");
    let first = 65580;
    let end = first + 2;
    let editor = OpenOptions::new()
        .read(true)
        .write(true)
        .open(&selected)
        .unwrap();
    let mut mapping = Mapping::new(&editor, original.len());
    mapping.write(first, &original[first..end]);
    let state = directory.path().join("private-state");
    Folder::initialize(&music, &state).await.unwrap();
    let app = Folder::open(&music, &state).await.unwrap().router();
    let page = support::search(&app, title, None, 1).await;
    page.validate(1).unwrap();
    assert_eq!(page.items.len(), 1);
    let response = support::post(
        &app,
        "/v1/sources",
        serde_json::json!({"recording_id":page.items[0].id}),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let sources: SourcePage =
        serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes()).unwrap();
    sources.validate().unwrap();
    assert_eq!(sources.items.len(), 1);
    assert!(sources.items[0].ready);
    let uri = format!("/v1/audio?source_id={}", sources.items[0].id);
    let response = app
        .clone()
        .oneshot(Request::get(&uri).body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let baseline = response.into_body().collect().await.unwrap().to_bytes();
    assert!(baseline.as_ref() == original.as_slice());

    let range = format!("bytes={first}-{}", end - 1);
    let response = app
        .clone()
        .oneshot(
            Request::get(&uri)
                .header("range", &range)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::PARTIAL_CONTENT);
    let baseline = response.into_body().collect().await.unwrap().to_bytes();
    assert!(baseline.as_ref() == &original[first..end]);
    let mut admitted = Vec::new();
    for (range, first, end, status) in [
        (None, 0, original.len(), StatusCode::OK),
        (
            Some(range.as_str()),
            first,
            end,
            StatusCode::PARTIAL_CONTENT,
        ),
    ] {
        let mut request = Request::get(&uri);
        if let Some(range) = range {
            request = request.header("range", range);
        }
        let response = app
            .clone()
            .oneshot(request.body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), status);
        admitted.push((response.into_body(), first, end, 0));
    }

    let response = app
        .clone()
        .oneshot(Request::get(&uri).body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let mut started = response.into_body();
    let prefix = started.frame().await.unwrap().unwrap().into_data().unwrap();
    assert!(!prefix.is_empty());
    assert!(prefix.len() <= 65536);
    assert!(prefix.len() <= first);
    assert!(prefix.as_ref() == &original[..prefix.len()]);
    admitted.push((started, 0, original.len(), prefix.len()));

    let before = file_stamp(&editor);
    let changed = [original[first] ^ 1, original[first + 1]];
    mapping.write(first, &changed);
    assert_eq!(
        file_stamp(&editor),
        before,
        "the retained editor mapping must leave indexed metadata unchanged"
    );
    assert_eq!(&fs::read(&selected).unwrap()[first..end], &changed);
    assert_ne!(&original[first..end], &changed);
    for (mut body, first, end, mut delivered) in admitted {
        let mut failed = false;
        while let Some(frame) = body.frame().await {
            match frame {
                Ok(frame) => {
                    if let Ok(bytes) = frame.into_data() {
                        assert!(bytes.len() <= end - first - delivered);
                        let last = first + delivered + bytes.len();
                        assert!(
                            bytes.as_ref() == &original[first + delivered..last],
                            "audio returned samples outside the indexed representation"
                        );
                        delivered += bytes.len();
                    }
                }
                Err(_) => {
                    failed = true;
                    break;
                }
            }
        }
        assert!(failed || delivered == end - first);
    }
}
