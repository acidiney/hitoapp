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

#[test]
fn changed_pcm_at_the_same_path_creates_a_new_recording_and_audio_representation() {
    let directory = tempfile::tempdir().unwrap();
    let music = directory.path().join("private-root");
    fs::create_dir(&music).unwrap();
    let file = music.join("private-reused.wav");
    let original = support::write_wav(&file, Some("Unchanged recording tags"), 440);
    assert_eq!(&original[36..40], b"data");
    let first = 44;
    let end = first + 32;
    let data_length = u32::from_le_bytes(original[40..44].try_into().unwrap()) as usize;
    let state = directory.path().join("private-state");
    binary::initialize(&music, &state);
    let service = binary::Service::start(&music, &state, &directory.path().join("before.jsonl"));
    let before: SearchPage = service.post(
        "/v1/search",
        serde_json::json!({"query":"Unchanged recording tags","limit":20}),
    );
    before.validate(20).unwrap();
    assert_eq!(before.items.len(), 1);
    let before = &before.items[0];
    let old_sources: SourcePage =
        service.post("/v1/sources", serde_json::json!({"recording_id":before.id}));
    old_sources.validate().unwrap();
    assert_eq!(old_sources.items.len(), 1);
    let old_source = &old_sources.items[0].id;
    let (status, body) = service.audio(old_source, first, end);
    assert_eq!(status, 206);
    assert_eq!(body, original[first..end]);
    service.stop();

    let current = support::write_wav(&file, Some("Unchanged recording tags"), 441);
    assert_eq!(current[..first], original[..first]);
    assert_eq!(
        current[first + data_length..],
        original[first + data_length..]
    );
    assert_ne!(current[first..end], original[first..end]);
    let service = binary::Service::start(&music, &state, &directory.path().join("after.jsonl"));
    let after: SearchPage = service.post(
        "/v1/search",
        serde_json::json!({"query":"Unchanged recording tags","limit":20}),
    );
    after.validate(20).unwrap();
    assert_eq!(after.items.len(), 1);
    let after = &after.items[0];
    assert_ne!(after.id, before.id);
    assert_eq!(after.title, before.title);
    assert_eq!(after.artist, before.artist);
    let sources: SourcePage =
        service.post("/v1/sources", serde_json::json!({"recording_id":after.id}));
    sources.validate().unwrap();
    assert_eq!(sources.items.len(), 1);
    assert!(sources.items[0].ready);
    assert_ne!(sources.items[0].id, *old_source);
    assert_eq!(service.audio(old_source, first, end).0, 404);
    let (status, body) = service.audio(&sources.items[0].id, first, end);
    assert_eq!(status, 206);
    assert_eq!(body, current[first..end]);
    service.stop();
}

#[test]
fn legacy_equal_pcm_aliases_retain_distinct_bindings_after_exact_byte_collision() {
    let directory = tempfile::tempdir().unwrap();
    let music = directory.path().join("private-root");
    fs::create_dir(&music).unwrap();
    let file_a = music.join("private-alias-a.wav");
    let file_b = music.join("private-alias-b.wav");
    let title_a = "Legacy alias A";
    let title_b = "Legacy alias B";
    let bytes_a = support::write_wav(&file_a, Some(title_a), 440);
    let bytes_b = support::write_wav(&file_b, Some(title_b), 440);
    let data_length = u32::from_le_bytes(bytes_a[40..44].try_into().unwrap()) as usize;
    assert_eq!(bytes_a[44..44 + data_length], bytes_b[44..44 + data_length]);
    assert_ne!(bytes_a, bytes_b);
    let first = bytes_a
        .windows(title_a.len())
        .position(|bytes| bytes == title_a.as_bytes())
        .unwrap();
    let end = first + title_a.len();
    assert_ne!(bytes_a[first..end], bytes_b[first..end]);
    let state = directory.path().join("private-state");
    binary::initialize(&music, &state);
    let service = binary::Service::start(&music, &state, &directory.path().join("before.jsonl"));
    let before: SearchPage = service.post(
        "/v1/search",
        serde_json::json!({"query":"Legacy alias","limit":20}),
    );
    before.validate(20).unwrap();
    assert_eq!(before.items.len(), 2);
    let recording_a = &before
        .items
        .iter()
        .find(|item| item.title == title_a)
        .unwrap()
        .id;
    let recording_b = &before
        .items
        .iter()
        .find(|item| item.title == title_b)
        .unwrap()
        .id;
    assert_ne!(recording_a, recording_b);
    assert_eq!(
        *recording_a,
        format!("recording-{:x}", Sha256::digest(&bytes_a))
    );
    assert_eq!(
        *recording_b,
        format!("recording-{:x}", Sha256::digest(&bytes_b))
    );
    let source_page = |service: &binary::Service, recording: &str| {
        let sources: SourcePage =
            service.post("/v1/sources", serde_json::json!({"recording_id":recording}));
        sources.validate().unwrap();
        assert_eq!(sources.items.len(), 1);
        assert!(sources.items[0].ready);
        sources.items[0].id.clone()
    };
    let old_source_a = source_page(&service, recording_a);
    let source_b = source_page(&service, recording_b);
    assert_ne!(old_source_a, source_b);
    assert_eq!(old_source_a, format!("source-{recording_a}"));
    assert_eq!(source_b, format!("source-{recording_b}"));
    assert_eq!(
        service.audio(&old_source_a, first, end),
        (206, bytes_a[first..end].to_vec())
    );
    assert_eq!(
        service.audio(&source_b, first, end),
        (206, bytes_b[first..end].to_vec())
    );
    service.stop();

    use std::os::unix::fs::MetadataExt;
    let inode = fs::metadata(&file_a).unwrap().ino();
    fs::write(&file_a, &bytes_b).unwrap();
    assert_eq!(fs::metadata(&file_a).unwrap().ino(), inode);
    assert_eq!(fs::read(&file_a).unwrap(), fs::read(&file_b).unwrap());
    assert_eq!(fs::read(&file_b).unwrap(), bytes_b);
    let service = binary::Service::start(&music, &state, &directory.path().join("after.jsonl"));
    let after: SearchPage = service.post(
        "/v1/search",
        serde_json::json!({"query":"Legacy alias","limit":20}),
    );
    after.validate(20).unwrap();
    assert_eq!(
        after
            .items
            .iter()
            .map(|item| item.id.clone())
            .collect::<BTreeSet<_>>(),
        BTreeSet::from([recording_a.clone(), recording_b.clone()])
    );
    for recording in [recording_a, recording_b] {
        let detail: hito_plugin_sdk::ProviderRecording = service.post(
            "/v1/recordings/detail",
            serde_json::json!({"recording_id":recording}),
        );
        assert_eq!(&detail.id, recording);
        assert_eq!(detail.title, title_b);
    }
    let current_source_a = source_page(&service, recording_a);
    let current_source_b = source_page(&service, recording_b);
    assert_ne!(current_source_a, old_source_a);
    assert_ne!(current_source_a, current_source_b);
    assert_eq!(current_source_b, source_b);
    assert_eq!(service.audio(&old_source_a, first, end).0, 404);
    for source in [current_source_a, current_source_b] {
        assert_eq!(
            service.audio(&source, first, end),
            (206, bytes_b[first..end].to_vec())
        );
    }
    service.stop();
}

#[test]
fn an_unknown_native_locator_with_ambiguous_historical_bytes_stays_separate_after_restart() {
    use std::{collections::BTreeMap, ffi::OsString, os::unix::ffi::OsStringExt};
    let directory = tempfile::tempdir().unwrap();
    let music = directory.path().join("private-root");
    fs::create_dir(&music).unwrap();
    let native_name =
        |suffix| OsString::from_vec([b"private-alias-".as_slice(), &[suffix], b".wav"].concat());
    let file_a = music.join(native_name(0xff));
    let file_b = music.join(native_name(0xfe));
    let bytes_a = support::write_wav(&file_a, Some("Ambiguous alias A"), 440);
    let bytes_b = support::write_wav(&file_b, Some("Ambiguous alias B"), 440);
    let recording_a = format!("recording-{:x}", Sha256::digest(&bytes_a));
    let recording_b = format!("recording-{:x}", Sha256::digest(&bytes_b));
    assert_ne!(recording_a, recording_b);
    let state = directory.path().join("private-state");
    binary::initialize(&music, &state);
    fs::write(&file_a, &bytes_b).unwrap();
    assert_eq!(fs::read(&file_a).unwrap(), fs::read(&file_b).unwrap());
    let source_for = |service: &binary::Service, recording: &str| {
        let sources: SourcePage =
            service.post("/v1/sources", serde_json::json!({"recording_id":recording}));
        sources.validate().unwrap();
        assert_eq!(sources.items.len(), 1);
        assert!(sources.items[0].ready);
        sources.items[0].id.clone()
    };
    let service = binary::Service::start(&music, &state, &directory.path().join("aliases.jsonl"));
    let aliases: SearchPage = service.post(
        "/v1/search",
        serde_json::json!({"query":"Ambiguous alias","limit":20}),
    );
    aliases.validate(20).unwrap();
    assert_eq!(
        aliases
            .items
            .iter()
            .map(|item| item.id.clone())
            .collect::<BTreeSet<_>>(),
        BTreeSet::from([recording_a.clone(), recording_b.clone()])
    );
    let mut expected_sources = BTreeMap::from([
        (recording_a.clone(), source_for(&service, &recording_a)),
        (recording_b.clone(), source_for(&service, &recording_b)),
    ]);
    assert_ne!(
        expected_sources[&recording_a],
        expected_sources[&recording_b]
    );
    service.stop();

    let unknown = music.join(native_name(0xfd));
    fs::write(&unknown, &bytes_b).unwrap();
    let first = bytes_b
        .windows(b"Ambiguous alias B".len())
        .position(|bytes| bytes == b"Ambiguous alias B")
        .unwrap();
    let end = first + b"Ambiguous alias B".len();
    for restart in 0..2 {
        let service = binary::Service::start(
            &music,
            &state,
            &directory.path().join(format!("unknown-{restart}.jsonl")),
        );
        let page: SearchPage = service.post(
            "/v1/search",
            serde_json::json!({"query":"Ambiguous alias","limit":20}),
        );
        page.validate(20).unwrap();
        assert_eq!(page.items.len(), 3);
        assert!(
            !serde_json::to_string(&page)
                .unwrap()
                .contains("private-alias")
        );
        let ids = page
            .items
            .iter()
            .map(|item| item.id.clone())
            .collect::<BTreeSet<_>>();
        assert!(ids.contains(&recording_a));
        assert!(ids.contains(&recording_b));
        if restart == 0 {
            let separate = ids
                .iter()
                .find(|id| !expected_sources.contains_key(*id))
                .unwrap();
            assert_ne!(separate, &recording_a);
            assert_ne!(separate, &recording_b);
            let source = source_for(&service, separate);
            assert!(
                expected_sources
                    .values()
                    .all(|previous| previous != &source)
            );
            expected_sources.insert(separate.clone(), source);
        }
        assert_eq!(
            ids,
            expected_sources.keys().cloned().collect::<BTreeSet<_>>()
        );
        for (recording, source) in &expected_sources {
            assert_eq!(source_for(&service, recording), *source);
            assert_eq!(
                service.audio(source, first, end),
                (206, bytes_b[first..end].to_vec())
            );
        }
        service.stop();
    }
}

#[test]
fn current_representations_keep_their_own_bytes_and_metadata_falls_back_when_owner_disappears() {
    let directory = tempfile::tempdir().unwrap();
    let music = directory.path().join("private-root");
    fs::create_dir(&music).unwrap();
    let incumbent = music.join("private-z-incumbent.wav");
    let original = support::write_wav(&incumbent, Some("Metadata owner Z"), 440);
    let first = original
        .windows(b"Metadata owner Z".len())
        .position(|bytes| bytes == b"Metadata owner Z")
        .unwrap();
    let end = first + b"Metadata owner Z".len();
    let state = directory.path().join("private-state");
    binary::initialize(&music, &state);
    let service = binary::Service::start(&music, &state, &directory.path().join("incumbent.jsonl"));
    let before: SearchPage = service.post(
        "/v1/search",
        serde_json::json!({"query":"Metadata owner","limit":20}),
    );
    before.validate(20).unwrap();
    assert_eq!(before.items.len(), 1);
    let recording = &before.items[0];
    assert_eq!(recording.title, "Metadata owner Z");
    assert!(recording.revision.is_some());
    let sources: SourcePage = service.post(
        "/v1/sources",
        serde_json::json!({"recording_id":recording.id}),
    );
    sources.validate().unwrap();
    assert_eq!(sources.items.len(), 1);
    let incumbent_source = &sources.items[0].id;
    assert!(sources.items[0].ready);
    assert_eq!(
        service.audio(incumbent_source, first, end),
        (206, original[first..end].to_vec())
    );
    service.stop();

    let alternate_folder = music.join("private-a-alternate");
    fs::create_dir(&alternate_folder).unwrap();
    let alternate = support::write_wav(
        &alternate_folder.join("copy.wav"),
        Some("Metadata owner A"),
        440,
    );
    assert_ne!(alternate[first..end], original[first..end]);
    let service = binary::Service::start(&music, &state, &directory.path().join("both.jsonl"));
    let both: SourcePage = service.post(
        "/v1/sources",
        serde_json::json!({"recording_id":recording.id}),
    );
    both.validate().unwrap();
    assert_eq!(
        both.items.len(),
        2,
        "complete page must include both current representations"
    );
    assert!(both.items.iter().all(|source| source.ready));
    assert!(
        both.items
            .iter()
            .any(|source| &source.id == incumbent_source)
    );
    let alternate_source = both
        .items
        .iter()
        .find(|source| &source.id != incumbent_source)
        .unwrap()
        .id
        .clone();
    assert_eq!(
        service.audio(incumbent_source, first, end),
        (206, original[first..end].to_vec())
    );
    assert_eq!(
        service.audio(&alternate_source, first, end),
        (206, alternate[first..end].to_vec())
    );
    let retained: hito_plugin_sdk::ProviderRecording = service.post(
        "/v1/recordings/detail",
        serde_json::json!({"recording_id":recording.id}),
    );
    assert_eq!(retained.id, recording.id);
    assert_eq!(retained.title, "Metadata owner Z");
    assert_eq!(retained.revision, recording.revision);
    service.stop();

    fs::remove_file(&incumbent).unwrap();
    let service = binary::Service::start(&music, &state, &directory.path().join("fallback.jsonl"));
    let after: hito_plugin_sdk::ProviderRecording = service.post(
        "/v1/recordings/detail",
        serde_json::json!({"recording_id":recording.id}),
    );
    assert_eq!(after.id, recording.id);
    assert_eq!(after.title, "Metadata owner A");
    assert!(after.revision.is_some());
    assert_ne!(after.revision, recording.revision);
    let current: SourcePage = service.post(
        "/v1/sources",
        serde_json::json!({"recording_id":recording.id}),
    );
    current.validate().unwrap();
    assert_eq!(current.items.len(), 1);
    assert_eq!(current.items[0].id, alternate_source);
    assert!(current.items[0].ready);
    assert_eq!(service.audio(incumbent_source, first, end).0, 404);
    assert_eq!(
        service.audio(&alternate_source, first, end),
        (206, alternate[first..end].to_vec())
    );
    let page: SearchPage = service.post(
        "/v1/search",
        serde_json::json!({"query":"Metadata owner","limit":20}),
    );
    page.validate(20).unwrap();
    assert_eq!(page.items.len(), 1);
    assert_eq!(page.items[0].id, after.id);
    assert_eq!(page.items[0].title, after.title);
    assert_eq!(page.items[0].revision, after.revision);
    service.stop();
}
