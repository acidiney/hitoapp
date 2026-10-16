#![cfg(unix)]
#[path = "support/binary.rs"]
mod binary;
#[allow(dead_code)]
mod support;

use hito_plugin_sdk::{SearchPage, SourcePage};
use sha2::{Digest, Sha256};
use std::{collections::BTreeSet, fs, os::unix::fs::PermissionsExt};

#[test]
fn explicit_initialization_preserves_legacy_wire_ids_across_binary_restarts() {
    let directory = tempfile::tempdir().unwrap();
    let music = directory.path().join("private-music");
    fs::create_dir(&music).unwrap();
    let state = directory.path().join("private-state");
    let mut expected = BTreeSet::new();
    for frequency in [440, 554, 659] {
        let bytes = support::write_wav(
            &music.join(format!("private-{frequency}.wav")),
            Some(&format!("Hito demo tone {frequency} Hz")),
            frequency,
        );
        expected.insert(format!("recording-{:x}", Sha256::digest(bytes)));
    }
    let release = format!(
        "release-{:x}",
        Sha256::digest(
            serde_json::to_vec(&("Owned test signals", "Hito synthetic demo", &expected,)).unwrap()
        )
    );
    binary::initialize(&music, &state);
    assert_eq!(
        fs::metadata(&state).unwrap().permissions().mode() & 0o777,
        0o700
    );
    assert_eq!(
        fs::metadata(state.join("identity.sqlite"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
    for restart in 0..2 {
        let service = binary::Service::start(
            &music,
            &state,
            &directory.path().join(format!("service-{restart}.jsonl")),
        );
        let page: SearchPage = service.post(
            "/v1/search",
            serde_json::json!({"query":"Hito demo","limit":20}),
        );
        page.validate(20).unwrap();
        assert_eq!(
            page.items
                .iter()
                .map(|item| item.id.clone())
                .collect::<BTreeSet<_>>(),
            expected
        );
        for recording in page.items {
            assert_eq!(recording.release.unwrap().id, release);
            let sources: SourcePage = service.post(
                "/v1/sources",
                serde_json::json!({"recording_id":recording.id}),
            );
            sources.validate().unwrap();
            assert_eq!(sources.items.len(), 1);
            assert_eq!(sources.items[0].id, format!("source-{}", recording.id));
            assert!(sources.items[0].ready);
        }
        service.stop();
    }
}

#[test]
fn verified_wav_retag_and_move_preserves_recording_but_retires_old_audio_representation() {
    let directory = tempfile::tempdir().unwrap();
    let music = directory.path().join("private-original-root");
    fs::create_dir(&music).unwrap();
    let original = music.join("private-original.wav");
    let original_bytes = support::write_wav(&original, Some("Blue retained recording"), 440);
    let first = original_bytes
        .windows(b"Blue retained recording".len())
        .position(|bytes| bytes == b"Blue retained recording")
        .unwrap();
    let end = first + 16;
    let state = directory.path().join("private-state");
    binary::initialize(&music, &state);
    let service = binary::Service::start(&music, &state, &directory.path().join("before.jsonl"));
    let before: SearchPage =
        service.post("/v1/search", serde_json::json!({"query":"Blue","limit":20}));
    before.validate(20).unwrap();
    assert_eq!(before.items.len(), 1);
    let before = &before.items[0];
    assert!(
        before
            .revision
            .as_deref()
            .is_some_and(|revision| !revision.is_empty())
    );
    let old_sources: SourcePage =
        service.post("/v1/sources", serde_json::json!({"recording_id":before.id}));
    assert_eq!(old_sources.items.len(), 1);
    let old_source = &old_sources.items[0].id;
    let (status, body) = service.audio(old_source, first, end);
    assert_eq!(status, 206);
    assert_eq!(body, original_bytes[first..end]);
    service.stop();

    let moved = directory.path().join("private-moved-root");
    fs::rename(&music, &moved).unwrap();
    let renamed = moved.join("private-renamed.wav");
    fs::rename(moved.join("private-original.wav"), &renamed).unwrap();
    let current_bytes = support::write_wav(&renamed, Some("Red retained recording"), 440);
    assert_ne!(current_bytes, original_bytes);
    assert_ne!(current_bytes[first..end], original_bytes[first..end]);
    let service = binary::Service::start(&moved, &state, &directory.path().join("after.jsonl"));
    let after: SearchPage =
        service.post("/v1/search", serde_json::json!({"query":"Red","limit":20}));
    after.validate(20).unwrap();
    assert_eq!(after.items.len(), 1);
    let after = &after.items[0];
    assert!(
        after
            .revision
            .as_deref()
            .is_some_and(|revision| !revision.is_empty())
    );
    assert_eq!(after.id, before.id);
    assert_ne!(after.revision, before.revision);
    let detail: hito_plugin_sdk::ProviderRecording = service.post(
        "/v1/recordings/detail",
        serde_json::json!({"recording_id":before.id}),
    );
    assert_eq!(detail.title, "Red retained recording");
    assert_eq!(detail.revision, after.revision);
    let sources: SourcePage =
        service.post("/v1/sources", serde_json::json!({"recording_id":before.id}));
    sources.validate().unwrap();
    assert_eq!(sources.items.len(), 1);
    assert!(sources.items[0].ready);
    assert_ne!(sources.items[0].id, *old_source);
    assert_eq!(service.audio(old_source, first, end).0, 404);
    let (status, body) = service.audio(&sources.items[0].id, first, end);
    assert_eq!(status, 206);
    assert_eq!(body, current_bytes[first..end]);
    let old_query: SearchPage =
        service.post("/v1/search", serde_json::json!({"query":"Blue","limit":20}));
    assert!(old_query.items.is_empty());
    service.stop();
}
