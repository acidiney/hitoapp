use hito_plugin_sdk::{
    Manifest, PlaylistPage, ProviderRecording, RecordingAttribution, RecordingMetadata, SearchPage,
};
use serde_json::{Value, json};

const LEGACY_RECORDING: &[u8] = br#"{"id":"recording:owned","title":"Owned recording","artist":"Owned performer","duration_ms":123000,"release":null,"disc_number":null,"track_number":null,"revision":null}"#;

fn attribution() -> Value {
    json!({
        "creators": ["Owned creator"],
        "provider_label": "Owned catalog",
        "content_url": "https://catalog.example/recordings/one"
    })
}

fn recording_with(attribution: Value) -> Value {
    let mut recording: Value = serde_json::from_slice(LEGACY_RECORDING).unwrap();
    recording["attribution"] = attribution;
    recording
}

#[test]
fn typed_attribution_round_trips_without_reordering_or_deduplicating_creator_names() {
    let expected = RecordingAttribution {
        creators: vec![
            "Second creator".into(),
            "First creator".into(),
            "Second creator".into(),
        ],
        provider_label: "Owned catalog".into(),
        content_url: "https://catalog.example/recordings/one".into(),
    };
    expected.validate().unwrap();
    let wire = br#"{"creators":["Second creator","First creator","Second creator"],"provider_label":"Owned catalog","content_url":"https://catalog.example/recordings/one"}"#;
    assert_eq!(serde_json::to_vec(&expected).unwrap(), wire);
    let decoded: RecordingAttribution = serde_json::from_slice(wire).unwrap();
    assert!(decoded == expected);
    let mut recording: ProviderRecording = serde_json::from_slice(LEGACY_RECORDING).unwrap();
    recording.attribution = Some(expected.clone());
    recording.validate().unwrap();
    let decoded: ProviderRecording =
        serde_json::from_slice(&serde_json::to_vec(&recording).unwrap()).unwrap();
    assert!(decoded.attribution == Some(expected));
    assert_eq!(decoded.artist, "Owned performer");
}

#[test]
fn absent_attribution_preserves_legacy_recording_page_metadata_and_manifest_fingerprint_input() {
    let recording: ProviderRecording = serde_json::from_slice(LEGACY_RECORDING).unwrap();
    recording.validate().unwrap();
    assert!(recording.attribution.is_none());
    assert_eq!(serde_json::to_vec(&recording).unwrap(), LEGACY_RECORDING);
    assert!(
        serde_json::to_value(&recording)
            .unwrap()
            .get("attribution")
            .is_none()
    );

    let legacy = std::str::from_utf8(LEGACY_RECORDING).unwrap();
    let search_wire = format!(r#"{{"items":[{legacy}],"next_cursor":null}}"#);
    let search: SearchPage = serde_json::from_str(&search_wire).unwrap();
    search.validate(1).unwrap();
    assert_eq!(serde_json::to_vec(&search).unwrap(), search_wire.as_bytes());

    let playlist_wire = format!(
        r#"{{"playlist_id":"playlist:owned","snapshot":"owned-snapshot","items":[{{"occurrence_id":"entry:owned","item":{{"kind":"recording","recording":{legacy}}}}}],"next_cursor":null}}"#
    );
    let playlist: PlaylistPage = serde_json::from_str(&playlist_wire).unwrap();
    playlist.validate(1).unwrap();
    assert_eq!(
        serde_json::to_vec(&playlist).unwrap(),
        playlist_wire.as_bytes()
    );

    let metadata_wire = format!(r#"{{"recording":{legacy}}}"#);
    let metadata: RecordingMetadata = serde_json::from_str(&metadata_wire).unwrap();
    metadata.validate().unwrap();
    assert_eq!(
        serde_json::to_vec(&metadata).unwrap(),
        metadata_wire.as_bytes()
    );

    let manifest_wire = br#"{"id":"org.hito.catalog-fixture","name":"Owned catalog fixture","protocol_version":1,"capabilities":["search","catalog","metadata","sources","playlists","artwork"],"content_policy":{"source_persistence":"explicit_download"}}"#;
    let manifest: Manifest = serde_json::from_slice(manifest_wire).unwrap();
    manifest.validate().unwrap();
    assert_eq!(serde_json::to_vec(&manifest).unwrap(), manifest_wire);
}

#[test]
fn an_explicit_null_attribution_serializes_as_absent_instead_of_null() {
    let recording: ProviderRecording = serde_json::from_value(recording_with(Value::Null)).unwrap();
    recording.validate().unwrap();
    assert!(recording.attribution.is_none());
    assert_eq!(serde_json::to_vec(&recording).unwrap(), LEGACY_RECORDING);
    assert!(
        serde_json::to_value(recording)
            .unwrap()
            .get("attribution")
            .is_none()
    );
}

#[test]
fn attribution_rejects_unknown_missing_and_malformed_nested_fields() {
    let mut unknown = attribution();
    unknown["audio_url"] = json!("https://media.example/private-transport");
    assert!(serde_json::from_value::<ProviderRecording>(recording_with(unknown)).is_err());
    for field in ["creators", "provider_label", "content_url"] {
        let mut missing = attribution();
        missing.as_object_mut().unwrap().remove(field);
        assert!(serde_json::from_value::<ProviderRecording>(recording_with(missing)).is_err());
    }
    for malformed in [json!("credits"), json!([]), json!(42), json!(true)] {
        assert!(serde_json::from_value::<ProviderRecording>(recording_with(malformed)).is_err());
    }
    for (field, malformed) in [
        ("creators", Value::Null),
        ("creators", json!("Owned creator")),
        ("creators", json!([null])),
        ("creators", json!([42])),
        ("provider_label", Value::Null),
        ("provider_label", json!(["Owned catalog"])),
        ("provider_label", json!(42)),
        ("content_url", Value::Null),
        ("content_url", json!([])),
        ("content_url", json!(true)),
    ] {
        let mut malformed_attribution = attribution();
        malformed_attribution[field] = malformed;
        assert!(
            serde_json::from_value::<ProviderRecording>(recording_with(malformed_attribution))
                .is_err()
        );
    }
}

#[test]
fn attribution_accepts_one_to_sixteen_ordered_creators_and_rejects_other_counts() {
    for count in [1, 16] {
        let mut value = attribution();
        value["creators"] = json!(vec!["Owned creator"; count]);
        let recording: ProviderRecording = serde_json::from_value(recording_with(value)).unwrap();
        recording.validate().unwrap();
        assert_eq!(recording.attribution.unwrap().creators.len(), count);
    }
    for count in [0, 17] {
        let mut value = attribution();
        value["creators"] = json!(vec!["Owned creator"; count]);
        let recording: ProviderRecording = serde_json::from_value(recording_with(value)).unwrap();
        assert_eq!(
            recording.validate().unwrap_err(),
            "Declare between one and sixteen recording creators"
        );
    }
}

#[test]
fn creator_names_use_utf8_byte_limits_and_preserve_valid_display_text() {
    for creator in [
        "Équipe de musique 演奏者".to_owned(),
        "Owned \"quoted\" creator \\ ensemble".to_owned(),
        "x".repeat(2048),
        "é".repeat(1024),
    ] {
        let mut value = attribution();
        value["creators"] = json!([creator]);
        let recording: ProviderRecording = serde_json::from_value(recording_with(value)).unwrap();
        recording.validate().unwrap();
        assert_eq!(recording.attribution.unwrap().creators, vec![creator]);
    }
    for creator in [
        String::new(),
        " \u{2003}".to_owned(),
        "Owned\ncreator".to_owned(),
        "Owned\tcreator".to_owned(),
        "Owned\0creator".to_owned(),
        "Owned\u{7f}creator".to_owned(),
        "x".repeat(2049),
        format!("{}x", "é".repeat(1024)),
    ] {
        let mut value = attribution();
        value["creators"] = json!([creator]);
        let recording: ProviderRecording = serde_json::from_value(recording_with(value)).unwrap();
        assert!(recording.validate().is_err());
    }
}

#[test]
fn provider_labels_use_utf8_byte_limits_and_reject_blank_or_control_text() {
    for label in ["Owned 音楽".to_owned(), "x".repeat(256), "é".repeat(128)] {
        let mut value = attribution();
        value["provider_label"] = json!(label);
        let recording: ProviderRecording = serde_json::from_value(recording_with(value)).unwrap();
        recording.validate().unwrap();
        assert_eq!(recording.attribution.unwrap().provider_label, label);
    }
    for label in [
        String::new(),
        " \u{2003}".to_owned(),
        "Owned\rlabel".to_owned(),
        "Owned\tlabel".to_owned(),
        "Owned\0label".to_owned(),
        "Owned\u{7f}label".to_owned(),
        "x".repeat(257),
        format!("{}x", "é".repeat(128)),
    ] {
        let mut value = attribution();
        value["provider_label"] = json!(label);
        let recording: ProviderRecording = serde_json::from_value(recording_with(value)).unwrap();
        assert!(recording.validate().is_err());
    }
}

#[test]
fn canonical_query_free_https_backlinks_accept_the_exact_url_byte_bound() {
    let prefix = "https://catalog.example/recordings/";
    let maximum = format!("{prefix}{}", "x".repeat(2048 - prefix.len()));
    assert_eq!(maximum.len(), 2048);
    for endpoint in [
        "https://catalog.example/recordings/one",
        "https://catalog.example/recordings/%C3%A9quipe",
        "https://catalog.example:8443/recordings/one",
        "https://youtu.be/HitoTone001",
        maximum.as_str(),
    ] {
        let mut value = attribution();
        value["content_url"] = json!(endpoint);
        let recording: ProviderRecording = serde_json::from_value(recording_with(value)).unwrap();
        recording.validate().unwrap();
        assert_eq!(recording.attribution.unwrap().content_url, endpoint);
    }
    let mut value = attribution();
    value["content_url"] = json!(format!("{maximum}x"));
    let recording: ProviderRecording = serde_json::from_value(recording_with(value)).unwrap();
    assert!(recording.validate().is_err());
}

#[test]
fn unsafe_or_noncanonical_backlinks_are_rejected_without_echoing_the_link() {
    for endpoint in [
        "",
        "/recordings/one",
        "//catalog.example/recordings/one",
        "http://catalog.example/recordings/one",
        "ftp://catalog.example/recordings/one",
        "file:///recordings/one",
        "javascript:alert('owned-secret')",
        "data:text/plain,owned-secret",
        "https://owned-user:owned-secret@catalog.example/recordings/one",
        "https://owned-user@catalog.example/recordings/one",
        "https://@catalog.example/recordings/one",
        "https://catalog.example/recordings/one?signature=owned-secret",
        "https://catalog.example/recordings/one?",
        "https://catalog.example/recordings/one#owned-secret",
        "https://catalog.example/recordings/one#",
        "https://catalog.example/recordings/owned secret",
        "https://catalog.example/recordings/owned\u{a0}secret",
        "https://catalog.example/recordings/owned\nsecret",
        "https://catalog.example/recordings/owned\tsecret",
        "https://catalog.example/recordings/owned\0secret",
        "https://catalog.example/recordings/owned\u{7f}secret",
        r"https:\\catalog.example\recordings\one",
        "https://catalog.example\\@other.example/recordings/one",
        "HTTPS://catalog.example/recordings/one",
        "https://CATALOG.example/recordings/one",
        "https://catalog.example:443/recordings/one",
        "https://catalog.example/recordings/../one",
        "https://catalog.example/recordings/%2e%2e/one",
        "https://catalog.example/recordings/équipe",
        "https://catalog.example",
        " https://catalog.example/recordings/one",
        "https://catalog.example/recordings/one ",
        "https:///catalog.example/recordings/one",
        "https://",
    ] {
        let mut value = attribution();
        value["content_url"] = json!(endpoint);
        let recording: ProviderRecording = serde_json::from_value(recording_with(value)).unwrap();
        let error = recording.validate().unwrap_err();
        assert_eq!(
            error,
            "Recording backlinks must use canonical HTTPS without credentials, query, or fragment"
        );
        assert!(error.len() <= 256 && !error.contains("owned-secret") && !error.contains("://"));
    }
}

#[cfg(feature = "schema")]
#[test]
fn attribution_schema_keeps_the_recording_field_optional_and_nested_fields_required() {
    use utoipa::OpenApi;

    #[derive(OpenApi)]
    #[openapi(components(schemas(ProviderRecording, RecordingAttribution)))]
    struct Contract;

    let document = serde_json::to_value(Contract::openapi()).unwrap();
    let schemas = &document["components"]["schemas"];
    assert!(
        !schemas["ProviderRecording"]["required"]
            .as_array()
            .unwrap()
            .contains(&json!("attribution"))
    );
    let attribution = &schemas["RecordingAttribution"];
    assert_eq!(attribution["additionalProperties"], json!(false));
    for field in ["creators", "provider_label", "content_url"] {
        assert!(
            attribution["required"]
                .as_array()
                .unwrap()
                .contains(&json!(field))
        );
    }
    assert_eq!(
        attribution["properties"]["creators"]["type"],
        json!("array")
    );
    assert_eq!(
        attribution["properties"]["creators"]["items"]["type"],
        json!("string")
    );
}
