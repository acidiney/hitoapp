#![cfg(unix)]
#[path = "support/binary.rs"]
mod binary;
#[allow(dead_code)]
mod support;

use hito_plugin_sdk::{SearchPage, SourcePage};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
};

#[test]
fn sixteen_complete_sources_survive_a_rejected_seventeenth_representation() {
    let directory = tempfile::tempdir().unwrap();
    let music = directory.path().join("private-root");
    fs::create_dir(&music).unwrap();
    let seed_title = "Representation 00";
    let seed = support::write_wav(&music.join("private-00.wav"), Some(seed_title), 440);
    assert_eq!(&seed[36..40], b"data");
    let pcm_end = 44 + u32::from_le_bytes(seed[40..44].try_into().unwrap()) as usize;
    let first = seed
        .windows(seed_title.len())
        .position(|bytes| bytes == seed_title.as_bytes())
        .unwrap();
    let end = first + seed_title.len();
    let state = directory.path().join("private-state");
    binary::initialize(&music, &state);
    let service = binary::Service::start(&music, &state, &directory.path().join("seed.jsonl"));
    let page: SearchPage = service.post(
        "/v1/search",
        serde_json::json!({"query":"Representation","limit":20}),
    );
    page.validate(20).unwrap();
    assert_eq!(page.items.len(), 1);
    let recording = &page.items[0].id;
    let initial: SourcePage =
        service.post("/v1/sources", serde_json::json!({"recording_id":recording}));
    initial.validate().unwrap();
    assert_eq!(initial.items.len(), 1);
    assert!(initial.items[0].ready);
    let seed_source = &initial.items[0].id;
    assert_eq!(
        service.audio(seed_source, first, end),
        (206, seed_title.as_bytes().to_vec())
    );
    service.stop();

    let mut expected = BTreeSet::from([seed_title.as_bytes().to_vec()]);
    for index in 1..16 {
        let title = format!("Representation {index:02}");
        let bytes = support::write_wav(
            &music.join(format!("private-{index:02}.wav")),
            Some(&title),
            440,
        );
        assert_eq!(bytes[44..pcm_end], seed[44..pcm_end]);
        assert_ne!(bytes, seed);
        assert_eq!(&bytes[first..end], title.as_bytes());
        assert!(expected.insert(bytes[first..end].to_vec()));
    }
    let service = binary::Service::start(&music, &state, &directory.path().join("sixteen.jsonl"));
    let page: SearchPage = service.post(
        "/v1/search",
        serde_json::json!({"query":"Representation","limit":20}),
    );
    page.validate(20).unwrap();
    assert_eq!(page.items.len(), 1);
    assert_eq!(page.items[0].id, *recording);
    let sources: SourcePage =
        service.post("/v1/sources", serde_json::json!({"recording_id":recording}));
    sources.validate().unwrap();
    assert_eq!(sources.items.len(), 16);
    let mut committed = BTreeMap::new();
    for source in sources.items {
        assert!(source.ready);
        let (status, bytes) = service.audio(&source.id, first, end);
        assert_eq!(status, 206);
        assert!(expected.contains(&bytes));
        assert!(committed.insert(source.id, bytes).is_none());
    }
    assert_eq!(committed.len(), 16);
    assert_eq!(
        committed.values().cloned().collect::<BTreeSet<_>>(),
        expected
    );
    assert_eq!(committed.get(seed_source).unwrap(), seed_title.as_bytes());
    service.stop();

    let excess = music.join("private-16.wav");
    let bytes = support::write_wav(&excess, Some("Representation 16"), 440);
    assert_eq!(bytes[44..pcm_end], seed[44..pcm_end]);
    assert_eq!(&bytes[first..end], b"Representation 16");
    binary::Service::reject_start(&music, &state, &directory.path().join("seventeen.jsonl"));
    fs::remove_file(excess).unwrap();

    let service = binary::Service::start(&music, &state, &directory.path().join("restored.jsonl"));
    let page: SearchPage = service.post(
        "/v1/search",
        serde_json::json!({"query":"Representation","limit":20}),
    );
    page.validate(20).unwrap();
    assert_eq!(page.items.len(), 1);
    assert_eq!(page.items[0].id, *recording);
    let sources: SourcePage =
        service.post("/v1/sources", serde_json::json!({"recording_id":recording}));
    sources.validate().unwrap();
    assert_eq!(sources.items.len(), 16);
    let mut reopened = BTreeMap::new();
    for source in sources.items {
        assert!(source.ready);
        let (status, bytes) = service.audio(&source.id, first, end);
        assert_eq!(status, 206);
        assert!(reopened.insert(source.id, bytes).is_none());
    }
    assert_eq!(reopened, committed);
    service.stop();
}
