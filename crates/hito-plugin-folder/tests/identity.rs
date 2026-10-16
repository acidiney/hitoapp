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
