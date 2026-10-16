#![cfg(unix)]
#[path = "support/binary.rs"]
mod binary;
#[allow(dead_code)]
mod support;

use hito_plugin_sdk::{ProviderRecording, SearchPage, SourcePage};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};

const TITLES: [&str; 2] = ["Topology track 01", "Topology track 02"];

#[derive(Clone, Debug, PartialEq, Eq)]
struct SourceBinding {
    recording: String,
    tags: Vec<u8>,
    pcm: Vec<u8>,
}

struct Current {
    recording: ProviderRecording,
    source: String,
    binding: SourceBinding,
}

type Snapshot = BTreeMap<String, Current>;

fn write_track(directory: &Path, index: usize, frequency: u32, album: &str) -> Vec<u8> {
    fs::create_dir_all(directory).unwrap();
    support::write_tagged_wav(
        &directory.join(format!("private-{index}.wav")),
        Some(TITLES[index]),
        frequency,
        "Topology performer",
        album,
    )
}

fn snapshot(service: &binary::Service, first: usize, end: usize) -> Snapshot {
    let page: SearchPage = service.post(
        "/v1/search",
        serde_json::json!({"query":"Topology track","limit":20}),
    );
    page.validate(20).unwrap();
    let mut snapshot = BTreeMap::new();
    for recording in page.items {
        assert!(
            recording
                .revision
                .as_deref()
                .is_some_and(|revision| !revision.is_empty())
        );
        let detail: ProviderRecording = service.post(
            "/v1/recordings/detail",
            serde_json::json!({"recording_id":recording.id}),
        );
        detail.validate().unwrap();
        assert_eq!(
            serde_json::to_value(&detail).unwrap(),
            serde_json::to_value(&recording).unwrap()
        );
        let sources: SourcePage = service.post(
            "/v1/sources",
            serde_json::json!({"recording_id":recording.id}),
        );
        sources.validate().unwrap();
        assert_eq!(sources.items.len(), 1);
        assert!(sources.items[0].ready);
        let source = sources.items.into_iter().next().unwrap().id;
        let (status, tags) = service.audio(&source, first, end);
        assert_eq!(status, 206);
        let (status, pcm) = service.audio(&source, 44, 76);
        assert_eq!(status, 206);
        let binding = SourceBinding {
            recording: recording.id.clone(),
            tags,
            pcm,
        };
        let title = recording.title.clone();
        assert!(
            snapshot
                .insert(
                    title,
                    Current {
                        recording,
                        source,
                        binding
                    }
                )
                .is_none()
        );
    }
    snapshot
}

fn unchanged(before: &Snapshot, after: &Snapshot) {
    assert_eq!(before.len(), after.len());
    for (title, expected) in before {
        let current = &after[title];
        assert_eq!(
            serde_json::to_value(&current.recording).unwrap(),
            serde_json::to_value(&expected.recording).unwrap()
        );
        assert_eq!(current.source, expected.source);
        assert_eq!(current.binding, expected.binding);
    }
}

fn observe_sources(
    service: &binary::Service,
    current: &Snapshot,
    history: &mut BTreeMap<String, SourceBinding>,
    first: usize,
    end: usize,
) {
    let active = current
        .values()
        .map(|track| track.source.clone())
        .collect::<BTreeSet<_>>();
    assert_eq!(active.len(), current.len());
    for track in current.values() {
        if let Some(previous) = history.get(&track.source) {
            assert_eq!(previous, &track.binding);
        } else {
            history.insert(track.source.clone(), track.binding.clone());
        }
    }
    for source in history.keys().filter(|source| !active.contains(*source)) {
        assert_eq!(service.audio(source, first, end).0, 404);
    }
}

#[test]
fn release_topology_preserves_recordings_and_retires_ambiguous_group_identities() {
    let directory = tempfile::tempdir().unwrap();
    let music = directory.path().join("private-root");
    let original = music.join("private-original");
    let frequencies = [440, 554];
    let seed_bytes = [
        write_track(&original, 0, frequencies[0], "Topology album 00"),
        write_track(&original, 1, frequencies[1], "Topology album 00"),
    ];
    assert_eq!(&seed_bytes[0][36..40], b"data");
    let pcm_end = 44 + u32::from_le_bytes(seed_bytes[0][40..44].try_into().unwrap()) as usize;
    assert_ne!(seed_bytes[0][44..pcm_end], seed_bytes[1][44..pcm_end]);
    let first = seed_bytes[0]
        .windows(TITLES[0].len())
        .position(|bytes| bytes == TITLES[0].as_bytes())
        .unwrap();
    let end = seed_bytes[0]
        .windows(b"Topology album 00".len())
        .position(|bytes| bytes == b"Topology album 00")
        .unwrap()
        + b"Topology album 00".len();
    let state = directory.path().join("private-state");
    binary::initialize(&music, &state);
    let mut history = BTreeMap::new();
    let service = binary::Service::start(&music, &state, &directory.path().join("seed.jsonl"));
    let seed = snapshot(&service, first, end);
    assert_eq!(seed.len(), 2);
    let original_release = seed[TITLES[0]]
        .recording
        .release
        .as_ref()
        .unwrap()
        .id
        .clone();
    for index in 0..2 {
        let track = &seed[TITLES[index]];
        assert_eq!(
            track.recording.release.as_ref().unwrap().id,
            original_release
        );
        assert_eq!(track.binding.tags, seed_bytes[index][first..end]);
        assert_eq!(track.binding.pcm, seed_bytes[index][44..76]);
    }
    observe_sources(&service, &seed, &mut history, first, end);
    service.stop();

    let split_directories = [music.join("private-split-a"), music.join("private-split-b")];
    let split_albums = ["Topology album 01", "Topology album 02"];
    let mut split_bytes = Vec::new();
    for index in 0..2 {
        fs::create_dir(&split_directories[index]).unwrap();
        fs::rename(
            original.join(format!("private-{index}.wav")),
            split_directories[index].join(format!("private-{index}.wav")),
        )
        .unwrap();
        let bytes = write_track(
            &split_directories[index],
            index,
            frequencies[index],
            split_albums[index],
        );
        assert_eq!(bytes[44..pcm_end], seed_bytes[index][44..pcm_end]);
        assert_ne!(bytes[first..end], seed_bytes[index][first..end]);
        split_bytes.push(bytes);
    }
    let service = binary::Service::start(&music, &state, &directory.path().join("split.jsonl"));
    let split = snapshot(&service, first, end);
    assert_eq!(split.len(), 2);
    let split_releases = split
        .values()
        .map(|track| track.recording.release.as_ref().unwrap().id.clone())
        .collect::<BTreeSet<_>>();
    assert_eq!(split_releases.len(), 2);
    assert!(!split_releases.contains(&original_release));
    for index in 0..2 {
        let track = &split[TITLES[index]];
        assert_eq!(track.recording.id, seed[TITLES[index]].recording.id);
        assert_eq!(
            track.recording.release.as_ref().unwrap().title,
            split_albums[index]
        );
        assert_ne!(track.source, seed[TITLES[index]].source);
        assert_eq!(track.binding.tags, split_bytes[index][first..end]);
        assert_eq!(track.binding.pcm, split_bytes[index][44..76]);
    }
    observe_sources(&service, &split, &mut history, first, end);
    service.stop();
    let service = binary::Service::start(
        &music,
        &state,
        &directory.path().join("split-unchanged.jsonl"),
    );
    let repeated = snapshot(&service, first, end);
    unchanged(&split, &repeated);
    observe_sources(&service, &repeated, &mut history, first, end);
    service.stop();

    let merged_directory = music.join("private-merged");
    fs::create_dir(&merged_directory).unwrap();
    let mut merged_bytes = Vec::new();
    for index in 0..2 {
        fs::rename(
            split_directories[index].join(format!("private-{index}.wav")),
            merged_directory.join(format!("private-{index}.wav")),
        )
        .unwrap();
        let bytes = write_track(
            &merged_directory,
            index,
            frequencies[index],
            "Topology album 03",
        );
        assert_eq!(bytes[44..pcm_end], seed_bytes[index][44..pcm_end]);
        assert_ne!(bytes[first..end], split_bytes[index][first..end]);
        merged_bytes.push(bytes);
    }
    let service = binary::Service::start(&music, &state, &directory.path().join("merged.jsonl"));
    let merged = snapshot(&service, first, end);
    assert_eq!(merged.len(), 2);
    let merged_release = merged[TITLES[0]]
        .recording
        .release
        .as_ref()
        .unwrap()
        .id
        .clone();
    assert_ne!(merged_release, original_release);
    assert!(!split_releases.contains(&merged_release));
    for index in 0..2 {
        let track = &merged[TITLES[index]];
        assert_eq!(track.recording.id, seed[TITLES[index]].recording.id);
        assert_eq!(track.recording.release.as_ref().unwrap().id, merged_release);
        assert_ne!(track.source, split[TITLES[index]].source);
        assert_eq!(track.binding.tags, merged_bytes[index][first..end]);
        assert_eq!(track.binding.pcm, merged_bytes[index][44..76]);
    }
    observe_sources(&service, &merged, &mut history, first, end);
    service.stop();
    let service = binary::Service::start(
        &music,
        &state,
        &directory.path().join("merged-unchanged.jsonl"),
    );
    let repeated = snapshot(&service, first, end);
    unchanged(&merged, &repeated);
    observe_sources(&service, &repeated, &mut history, first, end);
    service.stop();

    for index in 0..2 {
        fs::remove_file(merged_directory.join(format!("private-{index}.wav"))).unwrap();
    }
    let service =
        binary::Service::start(&music, &state, &directory.path().join("unavailable.jsonl"));
    let unavailable = snapshot(&service, first, end);
    assert!(unavailable.is_empty());
    for track in merged.values() {
        let detail: ProviderRecording = service.post(
            "/v1/recordings/detail",
            serde_json::json!({"recording_id":track.recording.id}),
        );
        detail.validate().unwrap();
        assert_eq!(
            serde_json::to_value(detail).unwrap(),
            serde_json::to_value(&track.recording).unwrap()
        );
        let sources: SourcePage = service.post(
            "/v1/sources",
            serde_json::json!({"recording_id":track.recording.id}),
        );
        sources.validate().unwrap();
        assert!(sources.items.is_empty());
    }
    observe_sources(&service, &unavailable, &mut history, first, end);
    service.stop();

    for (index, bytes) in merged_bytes.iter().enumerate() {
        fs::write(merged_directory.join(format!("private-{index}.wav")), bytes).unwrap();
    }
    let service = binary::Service::start(&music, &state, &directory.path().join("restored.jsonl"));
    let restored = snapshot(&service, first, end);
    assert_eq!(restored.len(), 2);
    for index in 0..2 {
        let track = &restored[TITLES[index]];
        assert_eq!(track.recording.id, seed[TITLES[index]].recording.id);
        assert_eq!(track.recording.release.as_ref().unwrap().id, merged_release);
        assert!(!split_releases.contains(&track.recording.release.as_ref().unwrap().id));
        assert_eq!(track.binding.tags, merged_bytes[index][first..end]);
        assert_eq!(track.binding.pcm, merged_bytes[index][44..76]);
    }
    observe_sources(&service, &restored, &mut history, first, end);
    service.stop();

    let mut replacement_bytes = Vec::new();
    for (index, frequency) in [441, 555].into_iter().enumerate() {
        let bytes = write_track(&merged_directory, index, frequency, "Topology album 03");
        assert_eq!(bytes[..44], merged_bytes[index][..44]);
        assert_eq!(bytes[pcm_end..], merged_bytes[index][pcm_end..]);
        for original in &seed_bytes {
            assert_ne!(bytes[44..pcm_end], original[44..pcm_end]);
            assert_ne!(bytes[44..76], original[44..76]);
        }
        replacement_bytes.push(bytes);
    }
    let service =
        binary::Service::start(&music, &state, &directory.path().join("no-witness.jsonl"));
    let replacement = snapshot(&service, first, end);
    assert_eq!(replacement.len(), 2);
    let new_release = replacement[TITLES[0]]
        .recording
        .release
        .as_ref()
        .unwrap()
        .id
        .clone();
    assert_ne!(new_release, merged_release);
    assert_ne!(new_release, original_release);
    assert!(!split_releases.contains(&new_release));
    let old_recordings = seed
        .values()
        .map(|track| track.recording.id.clone())
        .collect::<BTreeSet<_>>();
    for index in 0..2 {
        let track = &replacement[TITLES[index]];
        assert!(!old_recordings.contains(&track.recording.id));
        assert_eq!(track.recording.release.as_ref().unwrap().id, new_release);
        assert_eq!(
            track.recording.release.as_ref().unwrap().title,
            "Topology album 03"
        );
        assert_ne!(track.source, restored[TITLES[index]].source);
        assert_eq!(track.binding.tags, replacement_bytes[index][first..end]);
        assert_eq!(track.binding.pcm, replacement_bytes[index][44..76]);
    }
    observe_sources(&service, &replacement, &mut history, first, end);
    service.stop();
}
