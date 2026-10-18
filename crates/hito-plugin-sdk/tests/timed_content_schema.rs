#![cfg(feature = "schema")]

use hito_plugin_sdk::{
    BackingTracksCapabilities, BackingTracksRequest, BackingTracksResponse, LyricsCapabilities,
    LyricsRequest, LyricsResponse, ProviderRecording, ProviderRecordingRef, RecordingAttribution,
    TimedLyricLine, TimedLyrics,
};
use serde_json::{Value, json};
use utoipa::OpenApi;

#[derive(OpenApi)]
#[openapi(components(schemas(
    BackingTracksCapabilities,
    BackingTracksRequest,
    BackingTracksResponse,
    LyricsCapabilities,
    LyricsRequest,
    LyricsResponse,
    ProviderRecordingRef,
    TimedLyricLine,
    TimedLyrics,
    ProviderRecording,
    RecordingAttribution
)))]
struct TimedContentContract;

#[test]
fn generated_timed_content_schemas_keep_strict_objects_nullable_unknowns_and_shared_recordings() {
    let document = serde_json::to_value(TimedContentContract::openapi()).unwrap();
    let schemas = &document["components"]["schemas"];
    for (name, required) in [
        ("LyricsCapabilities", json!(["lyrics_version"])),
        (
            "BackingTracksCapabilities",
            json!(["backing_tracks_version"]),
        ),
        ("LyricsRequest", json!(["recording"])),
        ("LyricsResponse", json!(["recording"])),
        ("BackingTracksRequest", json!(["recording"])),
        ("BackingTracksResponse", json!(["recording", "items"])),
        ("ProviderRecordingRef", json!(["id"])),
        ("TimedLyrics", json!(["id", "rendition_id", "lines"])),
        ("TimedLyricLine", json!(["start_ms", "end_ms", "text"])),
    ] {
        assert_eq!(schemas[name]["additionalProperties"], json!(false));
        assert_eq!(schemas[name]["required"], required);
    }
    for (name, fields) in [
        ("ProviderRecordingRef", vec!["revision"]),
        ("LyricsResponse", vec!["lyrics"]),
        (
            "TimedLyrics",
            vec![
                "revision",
                "rendition_label",
                "language",
                "duration_ms",
                "attribution",
            ],
        ),
    ] {
        for field in fields {
            assert!(accepts_null(&schemas[name]["properties"][field]));
        }
    }
    for name in [
        "LyricsRequest",
        "LyricsResponse",
        "BackingTracksRequest",
        "BackingTracksResponse",
    ] {
        assert_eq!(
            schemas[name]["properties"]["recording"]["$ref"],
            json!("#/components/schemas/ProviderRecordingRef")
        );
        assert!(!accepts_null(&schemas[name]["properties"]["recording"]));
    }
    assert_eq!(
        schemas["TimedLyrics"]["properties"]["lines"]["items"]["$ref"],
        json!("#/components/schemas/TimedLyricLine")
    );
    assert_eq!(
        schemas["BackingTracksResponse"]["properties"]["items"]["items"]["$ref"],
        json!("#/components/schemas/ProviderRecording")
    );
    for field in ["start_ms", "end_ms"] {
        assert_eq!(
            schemas["TimedLyricLine"]["properties"][field]["type"],
            json!("integer")
        );
        assert!(!accepts_null(
            &schemas["TimedLyricLine"]["properties"][field]
        ));
    }
    assert!(!accepts_null(
        &schemas["TimedLyrics"]["properties"]["lines"]
    ));
    assert!(!accepts_null(
        &schemas["BackingTracksResponse"]["properties"]["items"]
    ));
    assert!(schemas.get("ProviderSource").is_none());
}

fn accepts_null(schema: &Value) -> bool {
    schema.get("type").is_some_and(|kind| {
        kind == "null"
            || kind
                .as_array()
                .is_some_and(|kinds| kinds.contains(&json!("null")))
    }) || ["anyOf", "oneOf"].iter().any(|key| {
        schema
            .get(key)
            .and_then(Value::as_array)
            .is_some_and(|choices| choices.iter().any(accepts_null))
    })
}
