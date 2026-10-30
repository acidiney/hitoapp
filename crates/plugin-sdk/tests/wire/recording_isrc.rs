use hito_plugin_sdk::{ProviderRecording, normalize_isrc};

fn recording(isrc: Option<&str>) -> ProviderRecording {
    serde_json::from_value(serde_json::json!({
        "id": "track-1", "title": "Title", "artist": "Artist", "duration_ms": null,
        "release": null, "disc_number": null, "track_number": null, "revision": null,
        "isrc": isrc,
    }))
    .unwrap()
}

#[test]
fn isrc_normalizes_case_and_hyphens_and_rejects_malformed_codes() {
    assert_eq!(
        normalize_isrc("USRC17607839").as_deref(),
        Some("USRC17607839")
    );
    assert_eq!(
        normalize_isrc("usrc17607839").as_deref(),
        Some("USRC17607839")
    );
    assert_eq!(
        normalize_isrc("US-RC1-76-07839").as_deref(),
        Some("USRC17607839")
    );
    assert_eq!(
        normalize_isrc("gbaaa2000001").as_deref(),
        Some("GBAAA2000001")
    );
    for invalid in [
        "",
        "USRC1760783",
        "USRC176078390",
        "1SRC17607839",
        "USRC1760783A",
        "US-RC17607839",
        "USRC1-76-07839",
        "US RC17607839",
        "USRC17607839\n",
        " USRC17607839",
        "ÜSRC17607839",
        "USRC1760783９",
    ] {
        assert_eq!(normalize_isrc(invalid), None, "{invalid:?}");
    }
}

#[test]
fn recordings_accept_an_optional_valid_isrc_and_old_payloads_omit_it() {
    assert!(recording(Some("us-rc1-76-07839")).validate().is_ok());
    assert_eq!(
        recording(Some("us-rc1-76-07839"))
            .normalized_isrc()
            .as_deref(),
        Some("USRC17607839")
    );
    assert!(recording(None).validate().is_ok());
    assert_eq!(recording(None).normalized_isrc(), None);
    assert!(recording(Some("not an isrc")).validate().is_err());

    let old: ProviderRecording = serde_json::from_str(
        r#"{"id":"t","title":"T","artist":"A","duration_ms":null,"release":null,"disc_number":null,"track_number":null,"revision":null}"#,
    )
    .unwrap();
    assert!(old.isrc.is_none());
    let encoded = serde_json::to_string(&old).unwrap();
    assert!(
        !encoded.contains("isrc"),
        "an absent ISRC keeps the earlier wire bytes: {encoded}"
    );
}
