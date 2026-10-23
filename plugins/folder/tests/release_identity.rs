#![cfg(unix)]
#[allow(dead_code)]
#[path = "support/binary.rs"]
mod binary;
#[allow(dead_code)]
mod support;

use hito_plugin_sdk::{ProviderRecording, SearchPage, SourcePage};
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
            assert_eq!(release.artist.as_deref(), Some(artist));
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

#[test]
fn a_surviving_release_occurrence_is_not_merged_with_a_removed_nonowner_group() {
    let directory = tempfile::tempdir().unwrap();
    let music = directory.path().join("private-root");
    fs::create_dir(&music).unwrap();
    let title_a = "Release occurrence A";
    let title_z = "Release occurrence Z";
    let bytes_a = support::write_tagged_wav(
        &music.join("private-a.wav"),
        Some(title_a),
        440,
        "Album performer A",
        "Occurrence album A",
    );
    let first = bytes_a
        .windows(title_a.len())
        .position(|bytes| bytes == title_a.as_bytes())
        .unwrap();
    let end = first + title_a.len();
    let pcm_end = 44 + u32::from_le_bytes(bytes_a[40..44].try_into().unwrap()) as usize;
    let state = directory.path().join("private-state");
    binary::initialize(&music, &state);
    let service =
        binary::Service::start(&music, &state, &directory.path().join("selected-a.jsonl"));
    let initial: SearchPage = service.post(
        "/v1/search",
        serde_json::json!({"query":"Release occurrence","limit":20}),
    );
    initial.validate(20).unwrap();
    assert_eq!(initial.items.len(), 1);
    let recording = &initial.items[0];
    assert_eq!(recording.title, title_a);
    let release = &recording.release.as_ref().unwrap().id;
    assert!(!release.is_empty());
    let source_page: SourcePage = service.post(
        "/v1/sources",
        serde_json::json!({"recording_id":recording.id}),
    );
    source_page.validate().unwrap();
    assert_eq!(source_page.items.len(), 1);
    assert!(source_page.items[0].ready);
    let source_a = &source_page.items[0].id;
    assert_eq!(
        service.audio(source_a, first, end),
        (206, bytes_a[first..end].to_vec())
    );
    service.stop();

    let other = music.join("private-other-group");
    fs::create_dir(&other).unwrap();
    let file_z = other.join("private-z.wav");
    let bytes_z = support::write_tagged_wav(
        &file_z,
        Some(title_z),
        440,
        "Album performer Z",
        "Occurrence album Z",
    );
    assert_eq!(bytes_z[44..pcm_end], bytes_a[44..pcm_end]);
    assert_eq!(&bytes_a[first..end], title_a.as_bytes());
    assert_eq!(&bytes_z[first..end], title_z.as_bytes());
    assert_ne!(bytes_z[first..end], bytes_a[first..end]);
    let service =
        binary::Service::start(&music, &state, &directory.path().join("both-groups.jsonl"));
    let both: SearchPage = service.post(
        "/v1/search",
        serde_json::json!({"query":"Release occurrence","limit":20}),
    );
    both.validate(20).unwrap();
    assert_eq!(both.items.len(), 1);
    assert_eq!(both.items[0].id, recording.id);
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
    assert_eq!(sources.items.len(), 2);
    assert!(sources.items.iter().all(|source| source.ready));
    assert!(sources.items.iter().any(|source| &source.id == source_a));
    let source_z = &sources
        .items
        .iter()
        .find(|source| &source.id != source_a)
        .unwrap()
        .id;
    assert_ne!(source_a, source_z);
    assert_eq!(
        service.audio(source_a, first, end),
        (206, bytes_a[first..end].to_vec())
    );
    assert_eq!(
        service.audio(source_z, first, end),
        (206, bytes_z[first..end].to_vec())
    );
    service.stop();

    fs::remove_file(file_z).unwrap();
    for restart in 0..2 {
        let service = binary::Service::start(
            &music,
            &state,
            &directory.path().join(format!("surviving-{restart}.jsonl")),
        );
        let detail: ProviderRecording = service.post(
            "/v1/recordings/detail",
            serde_json::json!({"recording_id":recording.id}),
        );
        detail.validate().unwrap();
        assert_eq!(detail.id, recording.id);
        assert_eq!(detail.title, title_a);
        assert_eq!(&detail.release.as_ref().unwrap().id, release);
        assert_eq!(
            serde_json::to_value(&detail).unwrap(),
            serde_json::to_value(recording).unwrap()
        );
        let page: SearchPage = service.post(
            "/v1/search",
            serde_json::json!({"query":"Release occurrence","limit":20}),
        );
        page.validate(20).unwrap();
        assert_eq!(page.items.len(), 1);
        assert_eq!(
            serde_json::to_value(&page.items[0]).unwrap(),
            serde_json::to_value(&detail).unwrap()
        );
        let sources: SourcePage = service.post(
            "/v1/sources",
            serde_json::json!({"recording_id":recording.id}),
        );
        sources.validate().unwrap();
        assert_eq!(sources.items.len(), 1);
        assert_eq!(&sources.items[0].id, source_a);
        assert!(sources.items[0].ready);
        assert_eq!(
            service.audio(source_a, first, end),
            (206, bytes_a[first..end].to_vec())
        );
        assert_eq!(service.audio(source_z, first, end).0, 404);
        service.stop();
    }
}
