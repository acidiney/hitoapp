#![cfg(unix)]
#[path = "support/binary.rs"]
#[allow(dead_code)]
mod binary;
#[allow(dead_code)]
mod support;

use hito_plugin_folder::Folder;
use hito_plugin_sdk::{ProviderRecording, SearchPage, SourcePage};
use std::{
    fs,
    os::unix::fs::{MetadataExt, PermissionsExt},
};

#[tokio::test]
async fn a_state_directory_under_a_writable_ancestor_is_rejected_without_changing_identity() {
    let directory = tempfile::tempdir().unwrap();
    fs::set_permissions(directory.path(), fs::Permissions::from_mode(0o755)).unwrap();
    let music = directory.path().join("private-music");
    fs::create_dir(&music).unwrap();
    let title = "Retained ancestry recording";
    support::write_wav(&music.join("private-recording.wav"), Some(title), 440);
    let parent = directory.path().join("shared-parent");
    fs::create_dir(&parent).unwrap();
    fs::set_permissions(&parent, fs::Permissions::from_mode(0o700)).unwrap();
    let state = parent.join("private-state");
    Folder::initialize(&music, &state).await.unwrap();

    let service = binary::Service::start(&music, &state, &directory.path().join("before.jsonl"));
    let before: SearchPage =
        service.post("/v1/search", serde_json::json!({"query":title,"limit":20}));
    before.validate(20).unwrap();
    assert_eq!(before.items.len(), 1);
    let recording = &before.items[0];
    let original_recording = serde_json::to_value(recording).unwrap();
    let sources: SourcePage = service.post(
        "/v1/sources",
        serde_json::json!({"recording_id":recording.id}),
    );
    sources.validate().unwrap();
    assert_eq!(sources.items.len(), 1);
    assert!(sources.items[0].ready);
    let original_source = sources.items[0].id.clone();
    service.stop();

    let database = state.join("identity.sqlite");
    let original_database = fs::read(&database).unwrap();
    let lock = state.join("owner.lock");
    let lock_metadata = fs::metadata(&lock).unwrap();
    let original_lock = (
        lock_metadata.dev(),
        lock_metadata.ino(),
        lock_metadata.mode(),
        lock_metadata.uid(),
        lock_metadata.len(),
        lock_metadata.modified().unwrap(),
    );
    let mut original_entries = fs::read_dir(&state)
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect::<Vec<_>>();
    original_entries.sort();

    fs::set_permissions(&parent, fs::Permissions::from_mode(0o777)).unwrap();
    let parent_metadata = fs::metadata(&parent).unwrap();
    let state_metadata = fs::metadata(&state).unwrap();
    assert_eq!(parent_metadata.uid(), state_metadata.uid());
    assert_eq!(state_metadata.uid(), unsafe { libc::geteuid() });
    assert_eq!(state_metadata.mode() & 0o777, 0o700);
    assert_eq!(parent_metadata.mode() & 0o1000, 0);
    assert_eq!(parent_metadata.mode() & 0o003, 0o003);
    assert_eq!(
        fs::metadata(directory.path()).unwrap().mode() & 0o001,
        0o001
    );

    let rejected = Folder::open(&music, &state).await;
    assert!(
        rejected.is_err(),
        "another UID can rename the private state entry through its writable non-sticky parent"
    );
    assert_eq!(fs::read(&database).unwrap(), original_database);
    let lock_metadata = fs::metadata(&lock).unwrap();
    assert_eq!(
        (
            lock_metadata.dev(),
            lock_metadata.ino(),
            lock_metadata.mode(),
            lock_metadata.uid(),
            lock_metadata.len(),
            lock_metadata.modified().unwrap(),
        ),
        original_lock
    );
    let mut entries = fs::read_dir(&state)
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect::<Vec<_>>();
    entries.sort();
    assert_eq!(entries, original_entries);

    fs::set_permissions(&parent, fs::Permissions::from_mode(0o700)).unwrap();
    let service = binary::Service::start(&music, &state, &directory.path().join("recovered.jsonl"));
    let recovered: ProviderRecording = service.post(
        "/v1/recordings/detail",
        serde_json::json!({"recording_id":recording.id}),
    );
    recovered.validate().unwrap();
    assert_eq!(serde_json::to_value(recovered).unwrap(), original_recording);
    let sources: SourcePage = service.post(
        "/v1/sources",
        serde_json::json!({"recording_id":recording.id}),
    );
    sources.validate().unwrap();
    assert_eq!(sources.items.len(), 1);
    assert_eq!(sources.items[0].id, original_source);
    assert!(sources.items[0].ready);
    service.stop();
}
