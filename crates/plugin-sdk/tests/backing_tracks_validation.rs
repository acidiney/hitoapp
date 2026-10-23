use hito_plugin_sdk::{BackingTracksRequest, BackingTracksResponse};
use serde_json::{Value, json};

#[test]
fn backing_candidates_are_bounded_unique_recordings_separate_from_the_original() {
    let request = request();
    for count in 0..=16 {
        let wire = response(count);
        let decoded: BackingTracksResponse = serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(decoded.validate_for(&request), Ok(()));
        assert_eq!(decoded.items.len(), count);
        assert_eq!(serde_json::to_value(&decoded).unwrap(), wire);
        for recording in &decoded.items {
            recording.validate().unwrap();
            assert_ne!(recording.id, request.recording.id);
            assert_eq!(recording.duration_ms, None);
            assert!(recording.release.is_none());
            assert_eq!(recording.disc_number, None);
            assert_eq!(recording.track_number, None);
            assert_eq!(recording.revision, None);
            assert!(recording.attribution.is_none());
        }
    }
    assert_eq!(
        validate(&request, response(17)),
        Err("The provider returned too many backing recordings")
    );

    let mut duplicate = response(2);
    duplicate["items"][1]["id"] = duplicate["items"][0]["id"].clone();
    duplicate["items"][1]["title"] = json!("Another owned label");
    duplicate["items"][1]["revision"] = json!("another:version");
    assert_eq!(
        validate(&request, duplicate),
        Err("The provider returned duplicate backing recording IDs")
    );

    let mut original = response(1);
    original["items"][0]["id"] = json!(request.recording.id);
    original["items"][0]["revision"] = json!("different:version");
    assert_eq!(
        validate(&request, original),
        Err("Backing tracks must use recording identities distinct from the original")
    );
}

#[test]
fn backing_lookup_preserves_exact_original_versions_and_validates_nested_recordings() {
    for revision in [None, Some("owned:revision")] {
        for count in [0, 1] {
            let mut exact = response(count);
            exact["recording"]["revision"] = json!(revision);
            let request: BackingTracksRequest = serde_json::from_value(json!({
                "recording": exact["recording"]
            }))
            .unwrap();
            assert_eq!(request.validate(), Ok(()));
            assert_eq!(validate(&request, exact.clone()), Ok(()));
            for (field, changed) in [
                ("id", json!("original:owned?edition=live")),
                (
                    "revision",
                    if revision.is_none() {
                        json!("owned:revision")
                    } else {
                        Value::Null
                    },
                ),
                ("revision", json!("another:revision")),
            ] {
                let mut wrong = exact.clone();
                wrong["recording"][field] = changed;
                assert_eq!(
                    validate(&request, wrong),
                    Err("The provider returned backing tracks for a different recording version")
                );
            }
        }
    }

    let request = request();
    for (field, invalid, error) in [
        (
            "id",
            json!("x".repeat(257)),
            "Provider fields must use bounded nonempty text without control characters",
        ),
        (
            "title",
            json!(" "),
            "Provider fields must use bounded nonempty text without control characters",
        ),
        (
            "artist",
            json!("owned\nartist"),
            "Provider fields must use bounded nonempty text without control characters",
        ),
        (
            "revision",
            json!(""),
            "Provider fields must use bounded nonempty text without control characters",
        ),
        (
            "duration_ms",
            json!(u64::MAX),
            "The provider returned an unsupported recording duration",
        ),
        (
            "disc_number",
            json!(0),
            "Provider track and disc numbers must be positive",
        ),
        (
            "track_number",
            json!(0),
            "Provider track and disc numbers must be positive",
        ),
        (
            "release",
            json!({"id": "", "title": "Owned release"}),
            "Provider fields must use bounded nonempty text without control characters",
        ),
        (
            "attribution",
            json!({"creators": [], "provider_label": "Owned provider", "content_url": "https://backings.example/owned"}),
            "Declare between one and sixteen recording creators",
        ),
        (
            "attribution",
            json!({"creators": ["Owned creator"], "provider_label": "Owned provider", "content_url": "https://user:private@backings.example/owned"}),
            "Recording backlinks must use canonical HTTPS without credentials, query, or fragment",
        ),
    ] {
        let mut malformed = response(1);
        malformed["items"][0][field] = invalid;
        assert_eq!(validate(&request, malformed), Err(error));
    }
}

fn request() -> BackingTracksRequest {
    serde_json::from_value(json!({
        "recording": {"id": "original:owned", "revision": "owned:revision"}
    }))
    .unwrap()
}

fn response(count: usize) -> Value {
    let items: Vec<Value> = (0..count)
        .map(|index| {
            json!({
                "id": format!("backing:owned/{index}?rendition=instrumental~é"),
                "title": "Owned instrumental", "artist": "Owned ensemble", "duration_ms": null,
                "release": null, "disc_number": null, "track_number": null, "revision": null
            })
        })
        .collect();
    json!({
        "recording": {"id": "original:owned", "revision": "owned:revision"},
        "items": items
    })
}

fn validate(request: &BackingTracksRequest, wire: Value) -> Result<(), &'static str> {
    serde_json::from_value::<BackingTracksResponse>(wire)
        .unwrap()
        .validate_for(request)
}
