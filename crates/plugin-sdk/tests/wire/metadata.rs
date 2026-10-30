use hito_plugin_sdk::{
    MAX_RECORDING_METADATA_BYTES, METADATA_PATH, ProviderRecording, RecordingMetadata,
};
use serde_json::{Value, json};

fn recording() -> Value {
    json!({
        "id":"owned-recording", "title":"Owned video title", "artist":"Owned performer",
        "duration_ms":12500, "release":null, "disc_number":null, "track_number":null,
        "revision":null
    })
}

fn metadata() -> Value {
    json!({
        "recording":recording(), "musical_title":"Owned musical title",
        "album_title":"Live at the Owned test room", "source_ids":["owned-source"]
    })
}

fn accepted(value: Value) -> bool {
    serde_json::from_value::<RecordingMetadata>(value)
        .is_ok_and(|metadata| metadata.validate().is_ok())
}

#[test]
fn metadata_preserves_the_legacy_recording_wire_and_allows_metadata_without_sources() {
    assert!(METADATA_PATH == "v1/recordings/metadata");
    let legacy = r#"{"id":"owned-recording","title":"Owned video title","artist":"Owned performer","duration_ms":12500,"release":null,"disc_number":null,"track_number":null,"revision":null}"#;
    let original: ProviderRecording =
        serde_json::from_str(legacy).unwrap_or_else(|_| panic!("legacy recording fixture failed"));
    assert!(original.validate().is_ok());
    let response: RecordingMetadata = serde_json::from_value(json!({"recording":recording()}))
        .unwrap_or_else(|_| panic!("metadata-only fixture failed"));
    assert!(response.validate().is_ok());
    assert!(response.recording == original);
    assert!(
        response.musical_title.is_none()
            && response.album_title.is_none()
            && response.source_ids.is_none()
    );
    assert!(
        serde_json::to_string(&response.recording)
            .unwrap_or_else(|_| panic!("legacy serialization failed"))
            == legacy
    );
    assert!(
        serde_json::to_value(&response).unwrap_or_else(|_| panic!("metadata serialization failed"))
            == json!({"recording":recording()})
    );
    assert!(accepted(metadata()));
}

#[test]
fn metadata_rejects_unknown_shape_and_invalid_present_musical_text() {
    let mut extra = metadata();
    extra["stream_url"] = json!("must-not-be-accepted");
    assert!(!accepted(extra));
    let mut changed_legacy = metadata();
    changed_legacy["recording"]["musical_title"] = json!("must-not-be-accepted");
    assert!(!accepted(changed_legacy));
    let mut invalid_recording = metadata();
    invalid_recording["recording"]["title"] = json!(" ");
    assert!(!accepted(invalid_recording));
    for field in ["musical_title", "album_title"] {
        let mut maximum = metadata();
        maximum[field] = json!("é".repeat(1024));
        assert!(accepted(maximum), "bounded UTF-8 metadata was rejected");
        for invalid in [
            json!(" "),
            json!("Owned\ntext"),
            json!("é".repeat(1025)),
            json!(7),
        ] {
            let mut value = metadata();
            value[field] = invalid;
            assert!(
                !accepted(value),
                "invalid present metadata text was accepted"
            );
        }
    }
}

#[test]
fn metadata_source_witnesses_are_optional_unique_and_bounded() {
    let ids: Vec<String> = (0..16)
        .map(|index| format!("{index:03}{}", "x".repeat(253)))
        .collect();
    let mut maximum = metadata();
    maximum["source_ids"] = json!(ids);
    assert!(accepted(maximum));
    let mut too_many = ids.clone();
    too_many.push("owned-seventeenth-source".into());
    let mut duplicates = ids.clone();
    duplicates[15] = ids[0].clone();
    for invalid in [
        json!(too_many),
        json!(duplicates),
        json!([""]),
        json!(["owned\nsource"]),
        json!(["x".repeat(257)]),
        json!([null]),
        json!("not-a-list"),
    ] {
        let mut value = metadata();
        value["source_ids"] = invalid;
        assert!(!accepted(value), "invalid source witnesses were accepted");
    }
}

#[test]
fn metadata_limits_the_complete_encoded_response_to_32768_bytes() {
    assert_eq!(MAX_RECORDING_METADATA_BYTES, 32_768);
    let quoted = "\"".repeat(2048);
    let mut value = metadata();
    value["recording"]["id"] = json!("\"".repeat(256));
    value["recording"]["title"] = json!(quoted);
    value["recording"]["artist"] = json!(quoted);
    value["recording"]["revision"] = json!("\"".repeat(256));
    value["recording"]["release"] = json!({
        "id":"\"".repeat(256), "title":quoted, "artist":quoted
    });
    value["album_title"] = json!(quoted);
    value["musical_title"] = json!("m");
    value["source_ids"] = json!(
        (0..16)
            .map(|index| format!("{index:03}{}", "\"".repeat(253)))
            .collect::<Vec<_>>()
    );
    let mut response: RecordingMetadata =
        serde_json::from_value(value).unwrap_or_else(|_| panic!("encoded-bound fixture failed"));
    let length = serde_json::to_vec(&response)
        .unwrap_or_else(|_| panic!("encoded-bound serialization failed"))
        .len();
    assert!(
        length < MAX_RECORDING_METADATA_BYTES,
        "encoded-bound fixture starts too large"
    );
    let remaining = MAX_RECORDING_METADATA_BYTES - length;
    let musical_title = format!(
        "m{}{}",
        "\"".repeat(remaining / 2),
        "x".repeat(remaining % 2)
    );
    assert!(
        musical_title.len() < 2048,
        "encoded-bound fixture exceeds the text bound"
    );
    response.musical_title = Some(musical_title);
    assert_eq!(
        serde_json::to_vec(&response)
            .unwrap_or_else(|_| panic!("encoded-bound serialization failed"))
            .len(),
        32_768
    );
    assert!(
        response.validate().is_ok(),
        "response at the encoded bound was rejected"
    );
    response
        .musical_title
        .as_mut()
        .unwrap_or_else(|| panic!("encoded-bound musical title missing"))
        .push('x');
    assert_eq!(
        serde_json::to_vec(&response)
            .unwrap_or_else(|_| panic!("encoded-bound serialization failed"))
            .len(),
        32_769
    );
    assert!(
        response.validate().is_err(),
        "response beyond the encoded bound was accepted"
    );
}
