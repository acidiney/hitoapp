#![cfg(unix)]
#[path = "support/binary.rs"]
mod binary;
#[allow(dead_code)]
mod support;

use hito_plugin_sdk::{SearchPage, SourcePage};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    os::unix::fs::{DirBuilderExt, MetadataExt},
    path::Path,
};

fn copy_stopped_state(source: &Path, destination: &Path) {
    let metadata = fs::symlink_metadata(source).unwrap();
    if metadata.is_dir() {
        fs::DirBuilder::new()
            .mode(metadata.mode() & 0o7777)
            .create(destination)
            .unwrap();
        for entry in fs::read_dir(source).unwrap() {
            let entry = entry.unwrap();
            copy_stopped_state(&entry.path(), &destination.join(entry.file_name()));
        }
    } else {
        assert!(metadata.is_file());
        assert_eq!(fs::copy(source, destination).unwrap(), metadata.len());
    }
    fs::set_permissions(destination, metadata.permissions()).unwrap();
    let copied = fs::symlink_metadata(destination).unwrap();
    assert_eq!(copied.mode() & 0o7777, metadata.mode() & 0o7777);
    assert_eq!(copied.uid(), metadata.uid());
    assert_eq!(copied.gid(), metadata.gid());
}

#[test]
fn restored_private_state_rejects_an_issued_cursor_for_a_different_catalog_snapshot() {
    let directory = tempfile::tempdir().unwrap();
    let music = directory.path().join("private-root");
    fs::create_dir(&music).unwrap();
    let frequencies = [440, 554, 659];
    let fixtures = frequencies
        .iter()
        .enumerate()
        .map(|(index, frequency)| {
            let path = music.join(format!("private-{index}.wav"));
            let title = format!("Blue cursor track {index}");
            let bytes = support::write_wav(&path, Some(&title), *frequency);
            (path, title, bytes)
        })
        .collect::<Vec<_>>();
    let first = fixtures[0]
        .2
        .windows(fixtures[0].1.len())
        .position(|bytes| bytes == fixtures[0].1.as_bytes())
        .unwrap();
    let end = first + 16;
    let state = directory.path().join("private-state");
    let backup = directory.path().join("private-backup");
    binary::initialize(&music, &state);
    copy_stopped_state(&state, &backup);

    let service = binary::Service::start(&music, &state, &directory.path().join("issued.jsonl"));
    let issued: SearchPage = service.post(
        "/v1/search",
        serde_json::json!({"query":"Blue cursor","limit":1}),
    );
    issued.validate(1).unwrap();
    assert_eq!(issued.items.len(), 1);
    let old_cursor = issued
        .next_cursor
        .as_deref()
        .expect("remaining results need a cursor");
    let before: SearchPage = service.post(
        "/v1/search",
        serde_json::json!({"query":"Blue cursor","limit":20}),
    );
    before.validate(20).unwrap();
    assert_eq!(before.items.len(), 3);
    assert!(before.next_cursor.is_none());
    assert_eq!(issued.items[0].id, before.items[0].id);
    let mut sources = BTreeMap::new();
    for recording in &before.items {
        let index = fixtures
            .iter()
            .position(|(_, title, _)| title == &recording.title)
            .unwrap();
        let page: SourcePage = service.post(
            "/v1/sources",
            serde_json::json!({"recording_id":recording.id}),
        );
        page.validate().unwrap();
        assert_eq!(page.items.len(), 1);
        assert!(page.items[0].ready);
        assert_eq!(
            service.audio(&page.items[0].id, first, end),
            (206, fixtures[index].2[first..end].to_vec())
        );
        assert!(
            sources
                .insert(recording.id.clone(), page.items[0].id.clone())
                .is_none()
        );
    }
    service.stop();

    fs::rename(&state, directory.path().join("private-displaced-state")).unwrap();
    copy_stopped_state(&backup, &state);
    let changed = fixtures
        .iter()
        .position(|(_, title, _)| title == &issued.items[0].title)
        .unwrap();
    let changed_bytes = support::write_wav(
        &fixtures[changed].0,
        Some(&format!("Red cursor track {changed}")),
        frequencies[changed],
    );
    let old_bytes = &fixtures[changed].2;
    assert_eq!(&old_bytes[36..40], b"data");
    let pcm_end = 44 + u32::from_le_bytes(old_bytes[40..44].try_into().unwrap()) as usize;
    assert_eq!(changed_bytes[44..pcm_end], old_bytes[44..pcm_end]);
    assert_ne!(changed_bytes[first..end], old_bytes[first..end]);
    let service = binary::Service::start(&music, &state, &directory.path().join("restored.jsonl"));
    let current: SearchPage = service.post(
        "/v1/search",
        serde_json::json!({"query":"Blue cursor","limit":20}),
    );
    current.validate(20).unwrap();
    assert_eq!(current.items.len(), 2);
    assert!(current.next_cursor.is_none());
    let expected_ids = before
        .items
        .iter()
        .filter(|recording| recording.id != issued.items[0].id)
        .map(|recording| recording.id.clone())
        .collect::<BTreeSet<_>>();
    assert_eq!(
        current
            .items
            .iter()
            .map(|recording| recording.id.clone())
            .collect::<BTreeSet<_>>(),
        expected_ids
    );
    for recording in &current.items {
        let previous = before
            .items
            .iter()
            .find(|previous| previous.id == recording.id)
            .unwrap();
        assert_eq!(
            serde_json::to_value(recording).unwrap(),
            serde_json::to_value(previous).unwrap()
        );
        let index = fixtures
            .iter()
            .position(|(_, title, _)| title == &recording.title)
            .unwrap();
        let page: SourcePage = service.post(
            "/v1/sources",
            serde_json::json!({"recording_id":recording.id}),
        );
        page.validate().unwrap();
        assert_eq!(page.items.len(), 1);
        assert!(page.items[0].ready);
        assert_eq!(page.items[0].id, sources[&recording.id]);
        assert_eq!(
            service.audio(&page.items[0].id, first, end),
            (206, fixtures[index].2[first..end].to_vec())
        );
    }
    let retagged: SearchPage = service.post(
        "/v1/search",
        serde_json::json!({"query":"Red cursor","limit":20}),
    );
    retagged.validate(20).unwrap();
    assert_eq!(retagged.items.len(), 1);
    assert_eq!(retagged.items[0].id, issued.items[0].id);
    assert_ne!(retagged.items[0].revision, issued.items[0].revision);
    let page: SourcePage = service.post(
        "/v1/sources",
        serde_json::json!({"recording_id":issued.items[0].id}),
    );
    page.validate().unwrap();
    assert_eq!(page.items.len(), 1);
    assert!(page.items[0].ready);
    assert_ne!(page.items[0].id, sources[&issued.items[0].id]);
    assert_eq!(
        service.audio(&sources[&issued.items[0].id], first, end).0,
        404
    );
    assert_eq!(
        service.audio(&page.items[0].id, first, end),
        (206, changed_bytes[first..end].to_vec())
    );

    assert_eq!(
        service.post_status(
            "/v1/search",
            serde_json::json!({"query":"Blue cursor","limit":1,"cursor":old_cursor})
        ),
        400
    );
    let first_page: SearchPage = service.post(
        "/v1/search",
        serde_json::json!({"query":"Blue cursor","limit":1}),
    );
    first_page.validate(1).unwrap();
    assert_eq!(first_page.items.len(), 1);
    let fresh_cursor = first_page
        .next_cursor
        .as_deref()
        .expect("second current result needs a cursor");
    let second_page: SearchPage = service.post(
        "/v1/search",
        serde_json::json!({"query":"Blue cursor","limit":1,"cursor":fresh_cursor}),
    );
    second_page.validate(1).unwrap();
    assert_eq!(second_page.items.len(), 1);
    assert!(second_page.next_cursor.is_none());
    let paged = first_page
        .items
        .iter()
        .chain(&second_page.items)
        .map(|recording| serde_json::to_value(recording).unwrap())
        .collect::<Vec<_>>();
    let complete = current
        .items
        .iter()
        .map(|recording| serde_json::to_value(recording).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(paged, complete);
    assert_eq!(
        first_page
            .items
            .iter()
            .chain(&second_page.items)
            .map(|recording| &recording.id)
            .collect::<BTreeSet<_>>()
            .len(),
        2
    );
    service.stop();
}
