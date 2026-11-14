use hito_plugin_sdk::{
    Capability, MAX_SOURCE_LINK_REFERENCE_BYTES, Manifest, RECORDING_LOOKUP_PATH,
    RecordingLookupRequest, RecordingMetadata,
};
use serde_json::json;

#[test]
fn source_links_are_an_explicit_capability_that_requires_sources() {
    assert_eq!(RECORDING_LOOKUP_PATH, "v1/recordings/lookup");
    for (capabilities, accepted) in [
        (json!(["sources", "source_links"]), true),
        (
            json!(["search", "sources", "metadata", "source_links"]),
            true,
        ),
        (json!(["source_links"]), false),
        (json!(["metadata", "source_links"]), false),
    ] {
        let manifest: Manifest = serde_json::from_value(json!({
            "id": "org.hito.source-links",
            "name": "Owned link provider",
            "protocol_version": 1,
            "capabilities": capabilities
        }))
        .unwrap();
        assert_eq!(manifest.validate().is_ok(), accepted);
    }
    let legacy: Manifest = serde_json::from_value(json!({
        "id": "org.hito.legacy",
        "name": "Owned legacy provider",
        "protocol_version": 1,
        "capabilities": ["search", "sources"]
    }))
    .unwrap();
    legacy.validate().unwrap();
    assert!(!legacy.capabilities.contains(&Capability::SourceLinks));
    assert_eq!(
        serde_json::to_value(Capability::SourceLinks).unwrap(),
        json!("source_links")
    );
}

#[test]
fn lookup_requests_keep_one_bounded_opaque_reference() {
    assert_eq!(MAX_SOURCE_LINK_REFERENCE_BYTES, 2048);
    for reference in [
        "https://provider.example/owned/1".to_owned(),
        "owned-bare-id".to_owned(),
        "a".repeat(2048),
        "é".repeat(1024),
    ] {
        let wire = json!({"reference": reference});
        let request: RecordingLookupRequest = serde_json::from_value(wire.clone()).unwrap();
        request.validate().unwrap();
        assert_eq!(serde_json::to_value(&request).unwrap(), wire);
    }
    for reference in [
        String::new(),
        "   ".to_owned(),
        "a".repeat(2049),
        "é".repeat(1025),
        "owned\nlink".to_owned(),
        "owned\rlink".to_owned(),
        "owned\tlink".to_owned(),
        "owned\0link".to_owned(),
        "owned\u{7f}link".to_owned(),
        "owned\u{85}link".to_owned(),
    ] {
        let request = RecordingLookupRequest { reference };
        assert!(request.validate().is_err());
    }
    for extra in ["url", "installation_id", "headers"] {
        let mut wire = json!({"reference": "owned-bare-id"});
        wire[extra] = json!("owned-extra");
        assert!(serde_json::from_value::<RecordingLookupRequest>(wire).is_err());
    }
    assert!(serde_json::from_value::<RecordingLookupRequest>(json!({})).is_err());
    assert!(serde_json::from_value::<RecordingLookupRequest>(json!({"reference": 7})).is_err());
}

#[test]
fn lookup_answers_reuse_the_recording_metadata_contract() {
    let answer = json!({
        "recording": {
            "id": "owned-canonical", "title": "Owned title", "artist": "Owned artist",
            "duration_ms": 1000, "release": null, "disc_number": null,
            "track_number": null, "revision": null
        },
        "source_ids": ["owned-source"]
    });
    let metadata: RecordingMetadata = serde_json::from_value(answer.clone()).unwrap();
    metadata.validate().unwrap();
    assert_eq!(serde_json::to_value(&metadata).unwrap(), answer);
    let mut unknown = answer;
    unknown["reference"] = json!("owned-echo");
    assert!(serde_json::from_value::<RecordingMetadata>(unknown).is_err());
}
