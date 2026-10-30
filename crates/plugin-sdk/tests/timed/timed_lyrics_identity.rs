use hito_plugin_sdk::{LyricsRequest, LyricsResponse};
use serde_json::{Value, json};

const INVALID_TEXT: &str =
    "Provider fields must use bounded nonempty text without control characters";

#[test]
fn lyrics_preserve_exact_identity_and_validate_bounded_utf8_display_fields() {
    let mut response = valid_response();
    let request: LyricsRequest = serde_json::from_value(json!({
        "recording": response["recording"]
    }))
    .unwrap();
    for (path, maximum) in [
        ("/lyrics/id", 256),
        ("/lyrics/rendition_id", 256),
        ("/lyrics/revision", 256),
        ("/lyrics/rendition_label", 2048),
        ("/lyrics/language", 64),
        ("/lyrics/lines/0/text", 2048),
    ] {
        for valid in ["x".repeat(maximum), "é".repeat(maximum / 2)] {
            let mut bounded = response.clone();
            *bounded.pointer_mut(path).unwrap() = json!(valid);
            assert_eq!(validate(&request, bounded.clone()), Ok(()));
            let decoded: LyricsResponse = serde_json::from_value(bounded.clone()).unwrap();
            assert_eq!(serde_json::to_value(decoded).unwrap(), bounded);
        }
        for invalid in [
            String::new(),
            " \u{2003}".into(),
            "owned\nprivate".into(),
            "owned\0private".into(),
            "owned\u{7f}private".into(),
            "x".repeat(maximum + 1),
            format!("{}x", "é".repeat(maximum / 2)),
        ] {
            let mut malformed = response.clone();
            *malformed.pointer_mut(path).unwrap() = json!(invalid);
            assert_eq!(validate(&request, malformed), Err(INVALID_TEXT));
        }
    }

    for revision in [Value::Null, json!("owned:revision")] {
        response["recording"]["revision"] = revision.clone();
        let request: LyricsRequest = serde_json::from_value(json!({
            "recording": response["recording"]
        }))
        .unwrap();
        for lyrics in [response["lyrics"].clone(), Value::Null] {
            let mut exact = response.clone();
            exact["lyrics"] = lyrics;
            assert_eq!(validate(&request, exact.clone()), Ok(()));
            for (field, changed) in [
                ("id", json!("owned:recording?edition=live")),
                (
                    "revision",
                    if revision.is_null() {
                        json!("owned:revision")
                    } else {
                        Value::Null
                    },
                ),
                ("revision", json!("owned:other-revision")),
            ] {
                let mut wrong_version = exact.clone();
                wrong_version["recording"][field] = changed;
                assert_eq!(
                    validate(&request, wrong_version),
                    Err("The provider returned lyrics for a different recording version")
                );
            }
        }
    }

    for field in ["id", "revision"] {
        for valid in ["x".repeat(256), "é".repeat(128)] {
            let mut wire = json!({"recording": response["recording"]});
            wire["recording"][field] = json!(valid);
            let request: LyricsRequest = serde_json::from_value(wire).unwrap();
            assert_eq!(request.validate(), Ok(()));
            assert_eq!(
                LyricsResponse {
                    recording: request.recording.clone(),
                    lyrics: None
                }
                .validate_for(&request),
                Ok(())
            );
        }
        for invalid in [
            String::new(),
            " \u{2003}".into(),
            "owned\tprivate".into(),
            "x".repeat(257),
            format!("{}x", "é".repeat(128)),
        ] {
            let mut wire = json!({"recording": response["recording"]});
            wire["recording"][field] = json!(invalid);
            let request: LyricsRequest = serde_json::from_value(wire).unwrap();
            assert_eq!(request.validate(), Err(INVALID_TEXT));
            assert_eq!(
                LyricsResponse {
                    recording: request.recording.clone(),
                    lyrics: None
                }
                .validate_for(&request),
                Err(INVALID_TEXT)
            );
            let valid_request: LyricsRequest =
                serde_json::from_value(json!({"recording": response["recording"]})).unwrap();
            let mut invalid_response = response.clone();
            invalid_response["recording"][field] = json!(invalid);
            assert_eq!(
                validate(&valid_request, invalid_response),
                Err(INVALID_TEXT)
            );
        }
    }
}

#[test]
fn lyrics_validate_the_existing_recording_attribution_contract() {
    let response = valid_response();
    let request: LyricsRequest =
        serde_json::from_value(json!({"recording": response["recording"]})).unwrap();
    for (field, invalid, expected) in [
        (
            "creators",
            json!([]),
            "Declare between one and sixteen recording creators",
        ),
        (
            "creators",
            json!(vec!["owned"; 17]),
            "Declare between one and sixteen recording creators",
        ),
        ("creators", json!([" "]), INVALID_TEXT),
        ("creators", json!(["owned\ncreator"]), INVALID_TEXT),
        ("provider_label", json!("x".repeat(257)), INVALID_TEXT),
        ("provider_label", json!(""), INVALID_TEXT),
    ] {
        let mut malformed = response.clone();
        malformed["lyrics"]["attribution"][field] = invalid;
        assert_eq!(validate(&request, malformed), Err(expected));
    }
    for backlink in [
        "http://lyrics.example/owned",
        "https://user:private@lyrics.example/owned",
        "https://lyrics.example/owned?signature=private",
        "https://lyrics.example/owned#private",
        "https://LYRICS.example/owned",
        "https://lyrics.example/owned private",
    ] {
        let mut malformed = response.clone();
        malformed["lyrics"]["attribution"]["content_url"] = json!(backlink);
        assert_eq!(
            validate(&request, malformed),
            Err(
                "Recording backlinks must use canonical HTTPS without credentials, query, or fragment"
            )
        );
    }
    let mut no_attribution = response.clone();
    no_attribution["lyrics"]["attribution"] = Value::Null;
    assert_eq!(validate(&request, no_attribution), Ok(()));
}

fn validate(request: &LyricsRequest, wire: Value) -> Result<(), &'static str> {
    serde_json::from_value::<LyricsResponse>(wire)
        .unwrap()
        .validate_for(request)
}

fn valid_response() -> Value {
    json!({
        "recording": {"id": "owned:recording", "revision": "owned:revision"},
        "lyrics": {
            "id": "owned:lyrics", "rendition_id": "owned:rendition",
            "revision": "owned:document-revision", "rendition_label": "Owned album cut",
            "language": "pt-PT", "duration_ms": null,
            "attribution": {"creators": ["Owned creator"], "provider_label": "Owned provider", "content_url": "https://lyrics.example/owned"},
            "lines": [{"start_ms": 0, "end_ms": 1000, "text": "Owned lyric line"}]
        }
    })
}
