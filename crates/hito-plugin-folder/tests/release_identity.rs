#![cfg(unix)]
#[allow(dead_code)]
#[path = "support/binary.rs"]
mod binary;
#[allow(dead_code)]
mod support;

use hito_plugin_sdk::{ProviderRecording, SearchPage};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
};

#[test]
fn a_unique_recording_witness_preserves_release_identity_across_retags_moves_and_membership_changes()
 {
    let directory = tempfile::tempdir().unwrap();
    let music = directory.path().join("private-root");
    let group = music.join("private-original-group");
    fs::create_dir_all(&group).unwrap();
    let old_album = "Before witnessed album";
    let old_artist = "Before album ensemble";
    let first = support::write_tagged_wav(
        &group.join("private-a.wav"),
        Some("Witness member A"),
        440,
        old_artist,
        old_album,
    );
    let second = support::write_tagged_wav(
        &group.join("private-b.wav"),
        Some("Witness member B"),
        554,
        old_artist,
        old_album,
    );
    assert_eq!(&first[36..40], b"data");
    let pcm_end = 44 + u32::from_le_bytes(first[40..44].try_into().unwrap()) as usize;
    assert_ne!(first[44..pcm_end], second[44..pcm_end]);
    let expected = BTreeMap::from([
        (
            "Witness member A".to_owned(),
            format!("recording-{:x}", Sha256::digest(&first)),
        ),
        (
            "Witness member B".to_owned(),
            format!("recording-{:x}", Sha256::digest(&second)),
        ),
    ]);
    let legacy_release = format!(
        "release-{:x}",
        Sha256::digest(
            serde_json::to_vec(&(
                old_album,
                old_artist,
                expected.values().cloned().collect::<BTreeSet<_>>(),
            ))
            .unwrap()
        )
    );
    let state = directory.path().join("private-state");
    binary::initialize(&music, &state);
    let service = binary::Service::start(&music, &state, &directory.path().join("seed.jsonl"));
    let seed: SearchPage = service.post(
        "/v1/search",
        serde_json::json!({"query":old_album,"limit":20}),
    );
    seed.validate(20).unwrap();
    assert_eq!(seed.items.len(), 2);
    assert_eq!(seed.items[0].release.as_ref().unwrap().id, legacy_release);
    let members = |service: &binary::Service, album: &str, artist: &str| {
        let page: SearchPage =
            service.post("/v1/search", serde_json::json!({"query":album,"limit":20}));
        page.validate(20).unwrap();
        let mut members = BTreeMap::new();
        for recording in page.items {
            let release = recording.release.as_ref().unwrap();
            assert_eq!(release.id, legacy_release);
            assert_eq!(release.title, album);
            assert_eq!(release.artist, artist);
            let detail: ProviderRecording = service.post(
                "/v1/recordings/detail",
                serde_json::json!({"recording_id":recording.id}),
            );
            detail.validate().unwrap();
            assert_eq!(
                serde_json::to_value(&detail).unwrap(),
                serde_json::to_value(&recording).unwrap()
            );
            assert!(members.insert(recording.title, recording.id).is_none());
        }
        members
    };
    assert_eq!(members(&service, old_album, old_artist), expected);
    service.stop();

    let moved = music.join("private-moved-group");
    fs::rename(group, &moved).unwrap();
    let album = "After witnessed album";
    let artist = "After album ensemble";
    let current_first = support::write_tagged_wav(
        &moved.join("private-a.wav"),
        Some("Witness member A"),
        440,
        artist,
        album,
    );
    let current_second = support::write_tagged_wav(
        &moved.join("private-b.wav"),
        Some("Witness member B"),
        554,
        artist,
        album,
    );
    assert_ne!(current_first, first);
    assert_ne!(current_second, second);
    assert_eq!(current_first[44..pcm_end], first[44..pcm_end]);
    assert_eq!(current_second[44..pcm_end], second[44..pcm_end]);
    let service = binary::Service::start(
        &music,
        &state,
        &directory.path().join("retagged-moved.jsonl"),
    );
    assert_eq!(members(&service, album, artist), expected);
    let old_query: SearchPage = service.post(
        "/v1/search",
        serde_json::json!({"query":old_album,"limit":20}),
    );
    assert!(old_query.items.is_empty());
    service.stop();

    let third = support::write_tagged_wav(
        &moved.join("private-c.wav"),
        Some("Witness member C"),
        659,
        artist,
        album,
    );
    assert_ne!(third[44..pcm_end], first[44..pcm_end]);
    assert_ne!(third[44..pcm_end], second[44..pcm_end]);
    let service = binary::Service::start(&music, &state, &directory.path().join("added.jsonl"));
    let added = members(&service, album, artist);
    assert_eq!(added.len(), 3);
    for (title, recording) in &expected {
        assert_eq!(added.get(title), Some(recording));
    }
    let third_recording = added.get("Witness member C").unwrap();
    assert!(
        expected
            .values()
            .all(|recording| recording != third_recording)
    );
    service.stop();

    fs::remove_file(moved.join("private-a.wav")).unwrap();
    let service = binary::Service::start(&music, &state, &directory.path().join("removed.jsonl"));
    assert_eq!(
        members(&service, album, artist),
        BTreeMap::from([
            (
                "Witness member B".to_owned(),
                expected["Witness member B"].clone()
            ),
            ("Witness member C".to_owned(), third_recording.clone()),
        ])
    );
    service.stop();
}
