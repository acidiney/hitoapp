use hito_plugin_sdk::{
    Capability, DETAIL_PATH, HEALTH_PATH, HealthReport, METADATA_PATH, Manifest, PROTOCOL_VERSION,
    ProviderRecording, RecordingMetadata, RecordingRequest, SEARCH_PATH, SOURCES_PATH, SearchPage,
    SearchRequest, SourcePage,
};
use hito_testkit::fake_plugin::{FakePlugin, RecordedRequest, Reply};
use serde_json::json;

pub async fn start(id: &str, nonempty: bool, bearer: &'static str) -> FakePlugin {
    let detail: ProviderRecording = serde_json::from_value(json!({
        "id": "owned-authority-candidate", "title": "Owned tune",
        "artist": "Owned performer", "duration_ms": 1000,
        "release": null, "disc_number": null, "track_number": null,
        "revision": "owned-r1"
    }))
    .unwrap();
    assert!(detail.validate().is_ok());
    let page = SearchPage {
        items: if nonempty {
            vec![detail.clone()]
        } else {
            Vec::new()
        },
        next_cursor: None,
    };
    assert!(page.validate(20).is_ok());
    let sources: SourcePage = serde_json::from_value(json!({"items": [{
        "id": "owned-authority-source", "label": "Owned authority audio", "ready": true
    }]}))
    .unwrap();
    assert!(sources.validate().is_ok());
    let metadata = RecordingMetadata {
        recording: detail.clone(),
        musical_title: Some("Different owned tune".into()),
        album_title: Some("Owned album".into()),
        source_ids: Some(vec!["owned-authority-source".into()]),
    };
    assert!(metadata.validate().is_ok());
    let page = serde_json::to_value(page).unwrap();
    let detail = serde_json::to_value(detail).unwrap();
    let sources = serde_json::to_value(sources).unwrap();
    let metadata = serde_json::to_value(metadata).unwrap();
    FakePlugin::builder(Manifest {
        id: id.into(),
        name: "Owned source authority service".into(),
        protocol_version: PROTOCOL_VERSION,
        capabilities: [
            Capability::Search,
            Capability::Sources,
            Capability::Metadata,
        ]
        .into(),
        authorization: None,
        content_policy: None,
    })
    .require_protocol()
    .get(HEALTH_PATH, |_| {
        Reply::Json(serde_json::to_value(HealthReport::ok()).unwrap())
    })
    .post(SEARCH_PATH, move |request| {
        check_bearer(request, bearer);
        let search: SearchRequest = serde_json::from_value(request.json()).unwrap();
        assert!(search.validate().is_ok());
        assert_eq!(search.query, "Owned performer Owned tune");
        assert!(search.cursor.is_none());
        assert_eq!(search.limit, 20);
        Reply::Json(page.clone())
    })
    .post(DETAIL_PATH, move |request| {
        check_recording(request, bearer);
        Reply::Json(detail.clone())
    })
    .post(SOURCES_PATH, move |request| {
        check_recording(request, bearer);
        Reply::Json(sources.clone())
    })
    .post(METADATA_PATH, move |request| {
        check_recording(request, bearer);
        Reply::Json(metadata.clone())
    })
    .start()
    .await
}

fn check_bearer(request: &RecordedRequest, bearer: &str) {
    assert!(
        request
            .headers
            .get("authorization")
            .is_some_and(|header| header == format!("Bearer {bearer}").as_str())
    );
}

fn check_recording(request: &RecordedRequest, bearer: &str) {
    check_bearer(request, bearer);
    let request: RecordingRequest = serde_json::from_value(request.json()).unwrap();
    assert!(request.validate().is_ok());
    assert_eq!(request.recording_id, "owned-authority-candidate");
}
