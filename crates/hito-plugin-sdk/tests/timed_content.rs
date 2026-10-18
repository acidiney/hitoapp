use hito_plugin_sdk::{
    BACKING_TRACKS_CAPABILITIES_PATH, BACKING_TRACKS_PATH, BACKING_TRACKS_VERSION,
    BackingTracksCapabilities, BackingTracksRequest, BackingTracksResponse,
    LYRICS_CAPABILITIES_PATH, LYRICS_PATH, LYRICS_VERSION, LyricsCapabilities, LyricsRequest,
    LyricsResponse, Manifest, ProviderRecording, ProviderRecordingRef, RecordingAttribution,
    TimedLyricLine, TimedLyrics,
};
use serde_json::json;

#[test]
fn timed_content_preserves_exact_provider_versions_and_distinct_backing_recordings() {
    let legacy_bytes: &[u8] = br#"{"id":"org.hito.folder","name":"Music folder","protocol_version":1,"capabilities":["search","sources","lyrics","backing_tracks"]}"#;
    let legacy: Manifest = serde_json::from_slice(legacy_bytes)
        .expect("The existing manifest must deserialize unchanged");
    legacy
        .validate()
        .expect("The existing timed content capability declarations must remain valid");
    assert_eq!(
        serde_json::to_vec(&legacy).expect("The existing manifest must serialize"),
        legacy_bytes
    );

    assert_eq!(LYRICS_VERSION, 1);
    assert_eq!(LYRICS_CAPABILITIES_PATH, "v1/lyrics/capabilities");
    assert_eq!(LYRICS_PATH, "v1/lyrics");
    assert_eq!(BACKING_TRACKS_VERSION, 1);
    assert_eq!(
        BACKING_TRACKS_CAPABILITIES_PATH,
        "v1/backing-tracks/capabilities"
    );
    assert_eq!(BACKING_TRACKS_PATH, "v1/backing-tracks");

    let lyrics_descriptor: LyricsCapabilities = serde_json::from_value(json!({
        "lyrics_version": 1
    }))
    .expect("The independent lyrics descriptor must deserialize");
    lyrics_descriptor
        .validate()
        .expect("The first lyrics protocol version must be supported");
    assert_eq!(
        serde_json::to_value(&lyrics_descriptor).unwrap(),
        json!({"lyrics_version": 1})
    );
    let backing_descriptor: BackingTracksCapabilities = serde_json::from_value(json!({
        "backing_tracks_version": 1
    }))
    .expect("The independent backing tracks descriptor must deserialize");
    backing_descriptor
        .validate()
        .expect("The first backing tracks protocol version must be supported");
    assert_eq!(
        serde_json::to_value(&backing_descriptor).unwrap(),
        json!({"backing_tracks_version": 1})
    );

    let attribution = RecordingAttribution {
        creators: vec!["Owned lyric creator".into()],
        provider_label: "Owned lyrics provider".into(),
        content_url: "https://lyrics.example/recordings/owned".into(),
    };
    attribution
        .validate()
        .expect("The owned attribution must use a canonical safe backlink");
    let backing = ProviderRecording {
        id: "recording:owned/instrumental~á".into(),
        title: "Owned instrumental take".into(),
        artist: "Owned ensemble".into(),
        duration_ms: None,
        release: None,
        disc_number: None,
        track_number: None,
        revision: None,
        attribution: None,
    };
    backing
        .validate()
        .expect("The backing candidate must use the existing recording contract");

    for revision in [Some("owned-recording-revision:4"), None] {
        let recording = ProviderRecordingRef {
            id: "recording:owned/original?edition=album".into(),
            revision: revision.map(str::to_owned),
        };
        let request = LyricsRequest {
            recording: recording.clone(),
        };
        request
            .validate()
            .expect("The selected provider's opaque recording version must be valid");
        let request_wire = json!({
            "recording": {
                "id": "recording:owned/original?edition=album",
                "revision": revision
            }
        });
        assert_eq!(serde_json::to_value(&request).unwrap(), request_wire);

        let response = LyricsResponse {
            recording: recording.clone(),
            lyrics: Some(TimedLyrics {
                id: "lyrics:owned/first".into(),
                rendition_id: "rendition:owned/album-cut~á".into(),
                revision: None,
                rendition_label: None,
                language: Some("pt-PT".into()),
                duration_ms: None,
                attribution: Some(attribution.clone()),
                lines: vec![
                    TimedLyricLine {
                        start_ms: 0,
                        end_ms: 1000,
                        text: "Owned première phrase".into(),
                    },
                    TimedLyricLine {
                        start_ms: 2500,
                        end_ms: 4000,
                        text: "Owned dernière phrase".into(),
                    },
                ],
            }),
        };
        response
            .validate_for(&request)
            .expect("The exact provider version and sparse explicit intervals must be valid");
        let response_wire = json!({
            "recording": request_wire["recording"],
            "lyrics": {
                "id": "lyrics:owned/first",
                "rendition_id": "rendition:owned/album-cut~á",
                "revision": null,
                "rendition_label": null,
                "language": "pt-PT",
                "duration_ms": null,
                "attribution": attribution,
                "lines": [
                    {"start_ms": 0, "end_ms": 1000, "text": "Owned première phrase"},
                    {"start_ms": 2500, "end_ms": 4000, "text": "Owned dernière phrase"}
                ]
            }
        });
        assert_eq!(serde_json::to_value(&response).unwrap(), response_wire);
        let decoded: LyricsResponse = serde_json::from_value(response_wire.clone())
            .expect("The owned timed lyrics wire must deserialize");
        decoded
            .validate_for(&request)
            .expect("The timed lyrics wire must retain the requested provider version");
        assert_eq!(serde_json::to_value(&decoded).unwrap(), response_wire);

        let backing_request = BackingTracksRequest {
            recording: recording.clone(),
        };
        backing_request
            .validate()
            .expect("The original recording version must be valid for backing lookup");
        assert_eq!(
            serde_json::to_value(&backing_request).unwrap(),
            request_wire
        );
        let backings = BackingTracksResponse {
            recording,
            items: vec![backing.clone()],
        };
        backings
            .validate_for(&backing_request)
            .expect("A distinct instrumental recording must remain a separate candidate");
        assert_ne!(backings.items[0].id, backings.recording.id);
        let backing_wire = json!({
            "recording": request_wire["recording"],
            "items": [backing]
        });
        assert_eq!(serde_json::to_value(&backings).unwrap(), backing_wire);
        let decoded: BackingTracksResponse = serde_json::from_value(backing_wire.clone())
            .expect("Backing candidates must reuse the existing recording wire");
        decoded
            .validate_for(&backing_request)
            .expect("Backing lookup must retain the exact original provider version");
        assert_eq!(serde_json::to_value(&decoded).unwrap(), backing_wire);
    }
}
