use hito_plugin_sdk::{
    BackingTracksCapabilities, BackingTracksRequest, BackingTracksResponse, LyricsCapabilities,
    LyricsRequest, LyricsResponse,
};
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

#[test]
fn timed_content_rejects_unknown_wire_fields_and_unsupported_extension_versions() {
    let recording = json!({"id": "owned:recording", "revision": null});
    let response = json!({
        "recording": recording,
        "lyrics": {
            "id": "owned:lyrics", "rendition_id": "owned:rendition",
            "revision": null, "rendition_label": null, "language": null,
            "duration_ms": null,
            "attribution": {"creators": ["Owned creator"], "provider_label": "Owned provider", "content_url": "https://lyrics.example/owned"},
            "lines": [{"start_ms": 0, "end_ms": 1000, "text": "Owned lyric line"}]
        }
    });
    reject_unknown::<LyricsResponse>(
        &response,
        &[
            "",
            "/recording",
            "/lyrics",
            "/lyrics/lines/0",
            "/lyrics/attribution",
        ],
    );
    let request = json!({"recording": recording});
    reject_unknown::<LyricsRequest>(&request, &["", "/recording"]);
    reject_unknown::<BackingTracksRequest>(&request, &["", "/recording"]);
    reject_unknown::<BackingTracksResponse>(
        &json!({"recording": recording, "items": []}),
        &["", "/recording"],
    );
    reject_unknown::<LyricsCapabilities>(&json!({"lyrics_version": 1}), &[""]);
    reject_unknown::<BackingTracksCapabilities>(&json!({"backing_tracks_version": 1}), &[""]);
    for version in [0, 2, u32::MAX] {
        let lyrics: LyricsCapabilities =
            serde_json::from_value(json!({"lyrics_version": version})).unwrap();
        assert_eq!(
            lyrics.validate(),
            Err("Unsupported lyrics protocol version")
        );
        let backings: BackingTracksCapabilities =
            serde_json::from_value(json!({"backing_tracks_version": version})).unwrap();
        assert_eq!(
            backings.validate(),
            Err("Unsupported backing tracks protocol version")
        );
    }
}

fn reject_unknown<T: DeserializeOwned>(wire: &Value, paths: &[&str]) {
    for path in paths {
        let mut malformed = wire.clone();
        malformed.pointer_mut(path).unwrap()["unsupported"] = json!("owned");
        assert!(serde_json::from_value::<T>(malformed).is_err());
    }
}
