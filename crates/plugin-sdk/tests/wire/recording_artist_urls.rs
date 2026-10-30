use hito_plugin_sdk::ProviderRecording;

fn recording(urls: &[&str]) -> ProviderRecording {
    serde_json::from_value(serde_json::json!({
        "id": "track-1", "title": "Title", "artist": "Artist", "duration_ms": null,
        "release": null, "disc_number": null, "track_number": null, "revision": null,
        "artist_urls": urls,
    }))
    .unwrap()
}

#[test]
fn credited_artist_pages_are_optional_canonical_https_urls() {
    assert!(recording(&[]).validate().is_ok());
    assert!(
        recording(&["https://open.spotify.com/artist/0000000000000000000001"])
            .validate()
            .is_ok()
    );
    let two = ["https://example.com/a", "https://example.com/b"];
    assert!(recording(&two).validate().is_ok());
    for invalid in [
        "http://example.com/a",
        "https://user:secret@example.com/a",
        "https://example.com:8443/a",
        "https://example.com/a?token=1",
        "https://example.com/a#frag",
        "https://EXAMPLE.com/a",
        "not a url",
        "https://example.com/a b",
        "spotify:artist:1",
    ] {
        assert!(recording(&[invalid]).validate().is_err(), "{invalid}");
    }
    assert!(
        recording(&["https://example.com/a", "https://example.com/a"])
            .validate()
            .is_err()
    );
    let many: Vec<String> = (0..9)
        .map(|index| format!("https://example.com/{index}"))
        .collect();
    let many: Vec<&str> = many.iter().map(String::as_str).collect();
    assert!(recording(&many).validate().is_err());
    let long = format!("https://example.com/{}", "a".repeat(512));
    assert!(recording(&[&long]).validate().is_err());
}

#[test]
fn old_payloads_have_no_artist_urls_and_keep_their_wire_bytes() {
    let old: ProviderRecording = serde_json::from_str(
        r#"{"id":"t","title":"T","artist":"A","duration_ms":null,"release":null,"disc_number":null,"track_number":null,"revision":null}"#,
    )
    .unwrap();
    assert!(old.artist_urls.is_empty());
    assert!(!serde_json::to_string(&old).unwrap().contains("artist_urls"));
}
