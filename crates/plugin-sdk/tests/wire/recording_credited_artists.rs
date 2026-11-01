use hito_plugin_sdk::{CreditRole, CreditedArtist, ProviderRecording};
use serde_json::{Value, json};

fn recording(artists: Value) -> ProviderRecording {
    serde_json::from_value(json!({
        "id": "track-1", "title": "Title", "artist": "A & B", "duration_ms": null,
        "release": null, "disc_number": null, "track_number": null, "revision": null,
        "artists": artists,
    }))
    .unwrap()
}

const PAGE: &str = "https://open.spotify.com/artist/0000000000000000000001";

#[test]
fn credited_artists_carry_names_roles_join_phrases_and_provider_identity() {
    let value = recording(json!([
        {"name": "A", "provider_artist_id": "0000000000000000000001", "page_url": PAGE, "join": " & "},
        {"name": "B", "role": "main", "join": " feat. "},
        {"name": "C", "role": "featured"},
    ]));
    value.validate().unwrap();
    assert_eq!(value.artists.len(), 3);
    assert_eq!(value.artists[0].role, CreditRole::Main);
    assert_eq!(value.artists[2].role, CreditRole::Featured);
    assert_eq!(value.artists[1].join.as_deref(), Some(" feat. "));
    let wire = serde_json::to_value(&value).unwrap();
    assert_eq!(wire["artists"][2]["role"], "featured");
    assert!(wire["artists"][1].get("provider_artist_id").is_none());
}

#[test]
fn old_payloads_have_no_credited_artists_and_keep_their_wire_bytes() {
    let old: ProviderRecording = serde_json::from_str(
        r#"{"id":"t","title":"T","artist":"A","duration_ms":null,"release":null,"disc_number":null,"track_number":null,"revision":null}"#,
    )
    .unwrap();
    assert!(old.artists.is_empty());
    old.validate().unwrap();
    assert!(!serde_json::to_string(&old).unwrap().contains("artists"));
}

#[test]
fn credited_artists_are_bounded_and_validated() {
    let valid = |name: &str| json!({"name": name});
    for invalid in [
        json!([{"name": "C", "role": "featured"}]),
        json!([{"name": ""}]),
        json!([{"name": "   "}]),
        json!([{"name": "A\u{0}"}]),
        json!([{"name": "A\u{202e}B"}]),
        json!([{"name": "x".repeat(257)}]),
        json!([valid("A"), valid("a")]),
        json!([valid("A"), {"name": "B", "role": "featured"}, {"name": "A", "role": "featured"}]),
        json!([{"name": "A", "provider_artist_id": "x"}, {"name": "B", "provider_artist_id": "x"}]),
        json!([{"name": "A", "provider_artist_id": ""}]),
        json!([{"name": "A", "provider_artist_id": "has space"}]),
        json!([{"name": "A", "provider_artist_id": "x".repeat(129)}]),
        json!([{"name": "A", "join": "x".repeat(33)}]),
        json!([{"name": "A", "join": "\n"}]),
        json!([{"name": "A", "page_url": "https://example.com/a"}]),
        json!([{"name": "A", "page_url": "http://open.spotify.com/artist/0000000000000000000001"}]),
        json!([{"name": "A", "role": "producer"}]),
    ] {
        let parsed = serde_json::from_value::<ProviderRecording>(json!({
            "id": "t", "title": "T", "artist": "A", "duration_ms": null, "release": null,
            "disc_number": null, "track_number": null, "revision": null, "artists": invalid,
        }));
        assert!(
            parsed.is_err() || parsed.unwrap().validate().is_err(),
            "{invalid}"
        );
    }
    let sixteen: Vec<Value> = (0..16)
        .map(|index| json!({"name": format!("A{index}")}))
        .collect();
    recording(Value::Array(sixteen.clone())).validate().unwrap();
    let mut seventeen = sixteen;
    seventeen.push(json!({"name": "A16"}));
    assert!(recording(Value::Array(seventeen)).validate().is_err());
    assert!(
        serde_json::from_value::<ProviderRecording>(json!({
            "id": "t", "title": "T", "artist": "A", "duration_ms": null, "release": null,
            "disc_number": null, "track_number": null, "revision": null,
            "artists": [{"name": "A", "unknown": 1}],
        }))
        .is_err()
    );
}

#[test]
fn constructing_a_credited_artist_by_hand_serializes_like_the_wire_form() {
    let artist = CreditedArtist {
        name: "A".into(),
        provider_artist_id: None,
        page_url: None,
        role: CreditRole::Main,
        join: None,
    };
    assert_eq!(
        serde_json::to_value(&artist).unwrap(),
        json!({"name": "A", "role": "main"})
    );
}
