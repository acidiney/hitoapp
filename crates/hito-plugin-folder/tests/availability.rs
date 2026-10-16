#![cfg(unix)]
#[path = "support/binary.rs"]
mod binary;
#[allow(dead_code)]
mod support;

use hito_plugin_sdk::{ProviderRecording, SearchPage, SourcePage};
use std::fs;

#[test]
fn a_known_recording_keeps_detail_and_returns_empty_complete_sources_after_its_last_locator_disappears()
 {
    let directory = tempfile::tempdir().unwrap();
    let music = directory.path().join("private-root");
    fs::create_dir(&music).unwrap();
    let first_copy = music.join("private-first.wav");
    let second_copy = music.join("private-second.wav");
    let bytes = support::write_wav(&first_copy, Some("Retained unavailable recording"), 440);
    fs::write(&second_copy, &bytes).unwrap();
    let first = bytes
        .windows(b"Retained unavailable recording".len())
        .position(|bytes| bytes == b"Retained unavailable recording")
        .unwrap();
    let end = first + 16;
    let state = directory.path().join("private-state");
    binary::initialize(&music, &state);
    let service = binary::Service::start(&music, &state, &directory.path().join("available.jsonl"));
    let page: SearchPage = service.post(
        "/v1/search",
        serde_json::json!({"query":"Retained unavailable","limit":20}),
    );
    page.validate(20).unwrap();
    assert_eq!(page.items.len(), 1);
    let retained = &page.items[0];
    let sources: SourcePage = service.post(
        "/v1/sources",
        serde_json::json!({"recording_id":retained.id}),
    );
    sources.validate().unwrap();
    assert_eq!(sources.items.len(), 1);
    assert!(sources.items[0].ready);
    let old_source = &sources.items[0].id;
    assert_eq!(
        service.audio(old_source, first, end),
        (206, bytes[first..end].to_vec())
    );
    service.stop();

    fs::remove_file(first_copy).unwrap();
    fs::remove_file(second_copy).unwrap();
    let service =
        binary::Service::start(&music, &state, &directory.path().join("unavailable.jsonl"));
    let detail: ProviderRecording = service.post(
        "/v1/recordings/detail",
        serde_json::json!({"recording_id":retained.id}),
    );
    assert_eq!(
        serde_json::to_value(&detail).unwrap(),
        serde_json::to_value(retained).unwrap()
    );
    let empty: SourcePage = service.post(
        "/v1/sources",
        serde_json::json!({"recording_id":retained.id}),
    );
    empty.validate().unwrap();
    assert!(
        empty.items.is_empty(),
        "known unavailable recording needs a complete empty current snapshot"
    );
    assert_eq!(service.audio(old_source, first, end).0, 404);
    service.stop();

    fs::write(music.join("private-restored.wav"), &bytes).unwrap();
    let service = binary::Service::start(&music, &state, &directory.path().join("restored.jsonl"));
    let restored: SearchPage = service.post(
        "/v1/search",
        serde_json::json!({"query":"Retained unavailable","limit":20}),
    );
    restored.validate(20).unwrap();
    assert_eq!(restored.items.len(), 1);
    assert_eq!(restored.items[0].id, retained.id);
    let sources: SourcePage = service.post(
        "/v1/sources",
        serde_json::json!({"recording_id":retained.id}),
    );
    sources.validate().unwrap();
    assert_eq!(sources.items.len(), 1);
    assert!(sources.items[0].ready);
    assert_eq!(
        service.audio(&sources.items[0].id, first, end),
        (206, bytes[first..end].to_vec())
    );
    service.stop();
}
