use hito_plugin_sdk::ProviderRecording;
use serde_json::{Value, json};

#[test]
fn release_credit_is_optional_but_present_credit_is_validated_without_changing_legacy_bytes() {
    let legacy = br#"{"id":"recording/owned","title":"Owned recording","artist":"Track performer","duration_ms":123000,"release":{"id":"album/verified","title":"Verified album","artist":"Verified album credit"},"disc_number":1,"track_number":2,"revision":"metadata-one"}"#;
    let known: ProviderRecording = serde_json::from_slice(legacy).unwrap();
    known.validate().unwrap();
    assert_eq!(serde_json::to_vec(&known).unwrap(), legacy);
    assert_eq!(
        known.release.as_ref().unwrap().artist.as_deref(),
        Some("Verified album credit")
    );

    let mut omitted: Value = serde_json::from_slice(legacy).unwrap();
    omitted["release"].as_object_mut().unwrap().remove("artist");
    let mut null = omitted.clone();
    null["release"]["artist"] = Value::Null;
    for wire in [omitted.clone(), null] {
        let unknown: ProviderRecording = serde_json::from_value(wire).unwrap();
        unknown.validate().unwrap();
        let release = unknown.release.as_ref().unwrap();
        assert_eq!(release.id, "album/verified");
        assert_eq!(release.title, "Verified album");
        assert!(release.artist.is_none());
        assert_eq!(unknown.artist, "Track performer");
        assert_eq!(serde_json::to_value(&unknown).unwrap(), omitted);
    }

    let mut maximum: Value = serde_json::from_slice(legacy).unwrap();
    maximum["release"]["artist"] = json!("é".repeat(1024));
    serde_json::from_value::<ProviderRecording>(maximum)
        .unwrap()
        .validate()
        .unwrap();
    for invalid in [json!(" "), json!("Owned\ncredit"), json!("é".repeat(1025))] {
        let mut wire: Value = serde_json::from_slice(legacy).unwrap();
        wire["release"]["artist"] = invalid;
        assert!(
            serde_json::from_value::<ProviderRecording>(wire)
                .unwrap()
                .validate()
                .is_err()
        );
    }
}
