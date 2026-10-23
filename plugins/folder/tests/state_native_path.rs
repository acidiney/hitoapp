#![cfg(target_os = "linux")]
#[path = "support/binary.rs"]
#[allow(dead_code)]
mod binary;
#[allow(dead_code)]
mod support;

use hito_plugin_folder::Folder;
use hito_plugin_sdk::{ProviderRecording, SearchPage, SourcePage};
use std::{
    ffi::OsString,
    fs,
    os::unix::{
        ffi::OsStringExt,
        fs::{DirBuilderExt, MetadataExt},
    },
};

#[derive(Debug, PartialEq)]
struct Snapshot {
    recording: serde_json::Value,
    sources: serde_json::Value,
}

fn snapshot(service: &binary::Service, title: &str, bytes: &[u8]) -> Snapshot {
    let page: SearchPage =
        service.post("/v1/search", serde_json::json!({"query":title,"limit":20}));
    page.validate(20).unwrap();
    assert_eq!(page.items.len(), 1);
    let recording = &page.items[0];
    assert_eq!(recording.title, title);
    assert_eq!(recording.artist, "Hito synthetic demo");
    assert_eq!(recording.duration_ms, Some(20_000));
    assert!(recording.revision.is_some());
    let release = recording.release.as_ref().unwrap();
    assert_eq!(release.title, "Owned test signals");
    let detail: ProviderRecording = service.post(
        "/v1/recordings/detail",
        serde_json::json!({"recording_id":recording.id}),
    );
    detail.validate().unwrap();
    assert_eq!(
        serde_json::to_value(&detail).unwrap(),
        serde_json::to_value(recording).unwrap()
    );
    let sources: SourcePage = service.post(
        "/v1/sources",
        serde_json::json!({"recording_id":recording.id}),
    );
    sources.validate().unwrap();
    assert_eq!(sources.items.len(), 1);
    assert!(sources.items[0].ready);
    let tag = bytes
        .windows(title.len())
        .position(|part| part == title.as_bytes())
        .unwrap();
    assert_eq!(&bytes[36..40], b"data");
    for range in [44..76, tag..tag + title.len()] {
        assert_eq!(
            service.audio(&sources.items[0].id, range.start, range.end),
            (206, bytes[range].to_vec())
        );
    }
    Snapshot {
        recording: serde_json::to_value(recording).unwrap(),
        sources: serde_json::to_value(sources).unwrap(),
    }
}

#[tokio::test]
async fn native_state_paths_preserve_public_identity_metadata_and_audio_across_restarts() {
    let directory = tempfile::tempdir().unwrap();
    let music = directory.path().join("private-music");
    fs::create_dir(&music).unwrap();
    let title = "Native state path recording";
    let bytes = support::write_wav(&music.join("private-recording.wav"), Some(title), 440);
    let parent = directory
        .path()
        .join(OsString::from_vec(b"private-parent-\xff".to_vec()));
    fs::DirBuilder::new().mode(0o700).create(&parent).unwrap();
    let metadata = fs::metadata(&parent).unwrap();
    assert_eq!(metadata.mode() & 0o777, 0o700);
    assert_eq!(metadata.uid(), unsafe { libc::geteuid() });
    assert!(parent.to_str().is_none());
    let state = parent.join(OsString::from_vec(b"private-state-\xfe".to_vec()));
    assert!(state.to_str().is_none());

    Folder::initialize(&music, &state).await.unwrap();
    let opened = Folder::open(&music, &state).await.unwrap();
    drop(opened);
    let service = binary::Service::start(
        &music,
        &state,
        &directory.path().join("native-before.jsonl"),
    );
    let before = snapshot(&service, title, &bytes);
    service.stop();

    let service =
        binary::Service::start(&music, &state, &directory.path().join("native-after.jsonl"));
    assert_eq!(snapshot(&service, title, &bytes), before);
    service.stop();
}
