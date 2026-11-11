use hito_plugin_sdk::{LyricsRequest, LyricsResponseV2};
use serde_json::json;

#[test]
fn plain_lyrics_preserve_stanza_boundaries_without_timing_or_invented_authors() {
    let request: LyricsRequest =
        serde_json::from_value(json!({"recording": {"id": "41", "revision": null}})).unwrap();
    let wire = json!({
        "recording": {"id": "41", "revision": null},
        "lyrics": {
            "id": "41", "rendition_id": "lrclib-record-41", "revision": null,
            "rendition_label": null, "language": null, "duration_ms": null,
            "attribution": {"provider_label": "LRCLIB", "content_url": "https://lrclib.net/api/get/41"},
            "content": {"kind": "plain", "lines": ["Synthetic river", "", "Owned horizon"]}
        }
    });
    let response: LyricsResponseV2 = serde_json::from_value(wire.clone()).unwrap();
    assert_eq!(response.validate_for(&request), Ok(()));
    assert_eq!(serde_json::to_value(response).unwrap(), wire);
}

fn anchored() -> serde_json::Value {
    json!({"recording": {"id": "41", "revision": null}, "lyrics": {
        "id": "owned-document", "rendition_id": "owned-timeline", "revision": null,
        "rendition_label": null, "language": null, "duration_ms": 200,
        "attribution": null, "content": {"kind": "anchored", "lines": [
            {"start_ms": 0, "text": "Synthetic river"},
            {"start_ms": 0, "text": "Parallel owned voice"},
            {"start_ms": 200, "text": ""}
        ]}
    }})
}

fn valid(wire: serde_json::Value) -> bool {
    let request =
        serde_json::from_value(json!({"recording": {"id": "41", "revision": null}})).unwrap();
    serde_json::from_value::<LyricsResponseV2>(wire)
        .is_ok_and(|response| response.validate_for(&request).is_ok())
}

#[test]
fn anchors_preserve_parallel_text_and_real_blank_clear_events_without_ends() {
    let wire = anchored();
    assert!(valid(wire.clone()));
    let response: LyricsResponseV2 = serde_json::from_value(wire.clone()).unwrap();
    assert_eq!(serde_json::to_value(response).unwrap(), wire);
    assert!(!wire.to_string().contains("end_ms"));
}

#[test]
fn invalid_identities_text_attribution_and_anchor_order_are_rejected() {
    let changes = vec![
        ("/recording/id", json!("42")),
        ("/recording/revision", json!("unreviewed")),
        ("/lyrics/id", json!(" ".repeat(257))),
        ("/lyrics/language", json!("x".repeat(65))),
        ("/lyrics/duration_ms", json!(604_800_001u64)),
        ("/lyrics/content/lines", json!([])),
        (
            "/lyrics/content/lines",
            json!([{"start_ms": 1, "text": ""}]),
        ),
        (
            "/lyrics/content/lines",
            json!([{"start_ms": 2, "text": "Owned"}, {"start_ms": 1, "text": "Earlier"}]),
        ),
        ("/lyrics/content/lines/0/start_ms", json!(-1)),
        ("/lyrics/content/lines/0/start_ms", json!(0.5)),
        ("/lyrics/content/lines/0/start_ms", json!(201)),
        ("/lyrics/content/lines/0/text", json!(" ".repeat(2049))),
        ("/lyrics/content/lines/0/text", json!("Owned\tprivate")),
        ("/lyrics/content/lines/0/text", json!("Owned\u{0000}")),
        (
            "/lyrics/attribution",
            json!({"provider_label": "LRCLIB", "content_url": "https://lrclib.net/api/get/41?private=x"}),
        ),
        (
            "/lyrics/attribution",
            json!({"provider_label": "LRCLIB", "content_url": "https://owner:secret@lrclib.net/41"}),
        ),
        (
            "/lyrics/attribution",
            json!({"provider_label": "LRCLIB", "content_url": "http://lrclib.net/41"}),
        ),
        (
            "/lyrics/attribution",
            json!({"provider_label": "LRCLIB", "content_url": "https://lrclib.net/41", "creators": vec!["Owned";17]}),
        ),
    ];
    for (path, value) in changes {
        let mut wire = anchored();
        *wire.pointer_mut(path).unwrap() = value;
        assert!(!valid(wire), "accepted invalid {path}");
    }
    for content in [
        json!({"kind":"plain","lines":[""]}),
        json!({"kind":"plain","lines":vec!["Owned";2001]}),
        json!({"kind":"plain","lines":["Owned"],"timestamps":[0]}),
    ] {
        let mut wire = anchored();
        wire["lyrics"]["content"] = content;
        assert!(!valid(wire));
    }
}

#[test]
fn the_complete_encoded_document_budget_counts_json_escaping() {
    let mut wire = anchored();
    wire["lyrics"]["content"] = json!({"kind":"plain","lines":vec!["\\".repeat(1024);127]});
    assert!(valid(wire.clone()));
    wire["lyrics"]["content"]["lines"]
        .as_array_mut()
        .unwrap()
        .push(json!("\\".repeat(1024)));
    assert!(!valid(wire));
}

#[test]
fn version_one_capability_and_null_response_bytes_stay_unchanged() {
    let capabilities = hito_plugin_sdk::LyricsCapabilities { lyrics_version: 1 };
    assert_eq!(
        serde_json::to_string(&capabilities).unwrap(),
        "{\"lyrics_version\":1}"
    );
    assert!(capabilities.validate().is_ok());
    let request: LyricsRequest =
        serde_json::from_value(json!({"recording":{"id":"41","revision":null}})).unwrap();
    let response = hito_plugin_sdk::LyricsResponse {
        recording: request.recording.clone(),
        lyrics: None,
    };
    assert_eq!(
        serde_json::to_string(&response).unwrap(),
        "{\"recording\":{\"id\":\"41\",\"revision\":null},\"lyrics\":null}"
    );
    assert_eq!(response.validate_for(&request), Ok(()));
}
