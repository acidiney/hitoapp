use hito_plugin_sdk::{
    CuratedCatalogError, CuratedEntryView, MAX_CURATED_CATALOG_BYTES, MAX_CURATED_ENTRIES,
    MAX_RECORDING_METADATA_BYTES, ProviderRecording, RecordingAttribution, admit_curated_catalog,
};

struct Entry {
    recording: ProviderRecording,
    source_id: String,
    label: String,
}

fn view(entry: &Entry) -> CuratedEntryView<'_> {
    CuratedEntryView {
        recording: &entry.recording,
        source_id: &entry.source_id,
        label: &entry.label,
    }
}

fn admit(entries: Vec<Entry>) -> Result<Vec<Entry>, CuratedCatalogError> {
    admit_curated_catalog(entries, view)
}

fn small(index: usize) -> Entry {
    Entry {
        recording: ProviderRecording {
            isrc: None,
            artist_urls: Vec::new(),
            id: format!("recording-{index:04}"),
            title: format!("Title {index}"),
            artist: "Artist".into(),
            duration_ms: None,
            release: None,
            disc_number: None,
            track_number: None,
            revision: None,
            attribution: None,
        },
        source_id: format!("source-{index:04}"),
        label: "Label".into(),
    }
}

fn sized(index: usize, bytes: usize) -> Entry {
    let mut entry = small(index);
    entry.recording.attribution = Some(Box::new(RecordingAttribution {
        creators: vec!["a".into()],
        provider_label: "owned-attribution".into(),
        content_url: "https://example.invalid/owned".into(),
    }));
    loop {
        let encoded = serde_json::to_vec(&entry.recording).unwrap().len();
        let missing = bytes.checked_sub(encoded).expect("the base recording fits");
        if missing == 0 {
            break;
        }
        let creators = &mut entry.recording.attribution.as_mut().unwrap().creators;
        match creators.iter_mut().find(|creator| creator.len() < 2048) {
            Some(creator) => creator.push_str(&"a".repeat(missing.min(2048 - creator.len()))),
            None => creators.push("a".into()),
        }
    }
    entry.recording.validate().unwrap();
    entry
}

fn error(entries: Vec<Entry>) -> CuratedCatalogError {
    match admit(entries) {
        Err(error) => error,
        Ok(_) => panic!("the catalog must be rejected"),
    }
}

#[test]
fn the_entry_count_message_names_the_limit() {
    assert_eq!(
        CuratedCatalogError::EntryCount.to_string(),
        format!("The curated catalog must contain 1 to {MAX_CURATED_ENTRIES} entries")
    );
}

#[test]
fn entry_count_must_be_between_one_and_the_limit() {
    assert_eq!(error(vec![]), CuratedCatalogError::EntryCount);
    assert_eq!(
        error((0..129).map(small).collect()),
        CuratedCatalogError::EntryCount
    );
    assert_eq!(admit((0..128).map(small).collect()).unwrap().len(), 128);
    assert_eq!(admit(vec![small(0)]).unwrap().len(), 1);
}

#[test]
fn admitted_entries_are_ordered_by_recording_id() {
    let admitted = admit(vec![small(3), small(1), small(2)]).unwrap();
    let ids: Vec<_> = admitted.iter().map(|e| e.recording.id.as_str()).collect();
    assert_eq!(ids, ["recording-0001", "recording-0002", "recording-0003"]);
}

#[test]
fn duplicate_recording_or_source_ids_are_rejected() {
    let mut same_recording = small(2);
    same_recording.recording.id = small(1).recording.id;
    assert_eq!(
        error(vec![small(1), same_recording]),
        CuratedCatalogError::DuplicateRecording
    );
    let mut same_source = small(2);
    same_source.source_id = small(1).source_id;
    assert_eq!(
        error(vec![small(1), same_source]),
        CuratedCatalogError::DuplicateSource
    );
}

#[test]
fn invalid_recording_or_source_fields_are_rejected() {
    let mut recording = small(1);
    recording.recording.title = " ".into();
    assert_eq!(
        error(vec![recording]),
        CuratedCatalogError::InvalidRecording
    );
    let mut source = small(1);
    source.label = String::new();
    assert_eq!(error(vec![source]), CuratedCatalogError::InvalidSource);
    let mut source = small(1);
    source.source_id = "x".repeat(257);
    assert_eq!(error(vec![source]), CuratedCatalogError::InvalidSource);
}

#[test]
fn one_recording_may_use_exactly_the_detail_budget() {
    let exact = sized(1, MAX_RECORDING_METADATA_BYTES);
    assert_eq!(admit(vec![exact]).unwrap().len(), 1);
    let over = sized(1, MAX_RECORDING_METADATA_BYTES + 1);
    assert_eq!(error(vec![over]), CuratedCatalogError::RecordingTooLarge);
}

#[test]
fn the_catalog_may_use_exactly_the_aggregate_budget() {
    let third = MAX_CURATED_CATALOG_BYTES / 3;
    assert_eq!(third * 3, MAX_CURATED_CATALOG_BYTES);
    let exact = vec![sized(1, third), sized(2, third), sized(3, third)];
    assert_eq!(admit(exact).unwrap().len(), 3);
    let over = vec![sized(1, third), sized(2, third), sized(3, third + 1)];
    assert_eq!(error(over), CuratedCatalogError::CatalogTooLarge);
}
