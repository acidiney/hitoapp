use hito_plugin_sdk::{
    BackingTracksRequest, BackingTracksResponse, ProviderRecording, ProviderRecordingRef,
    ProviderRelease, RecordingAttribution,
};

#[test]
fn complete_encoded_backings_accept_the_exact_budget_and_reject_one_more_valid_byte() {
    const MAXIMUM: usize = 262_144;
    let request = BackingTracksRequest {
        recording: ProviderRecordingRef {
            id: "é".repeat(128),
            revision: Some("r".repeat(256)),
        },
    };
    request.validate().unwrap();
    let mut response = BackingTracksResponse {
        recording: request.recording.clone(),
        items: (0..16)
            .map(|index| ProviderRecording {
                isrc: None,
                artist_urls: Vec::new(),
                id: format!("backing:owned/{index}/{}", "é".repeat(110)),
                title: "\"".repeat(1024),
                artist: "é".repeat(1024),
                duration_ms: Some(200_000),
                release: Some(ProviderRelease {
                    id: "a".repeat(256),
                    title: "é".repeat(1024),
                    artist: Some("\\".repeat(1024)),
                }),
                disc_number: Some(1),
                track_number: Some(index + 1),
                revision: Some("v".repeat(256)),
                attribution: Some(Box::new(RecordingAttribution {
                    creators: vec!["Owned é \"creator\" \\ ensemble".into(); 16],
                    provider_label: "p".repeat(256),
                    content_url: "https://backings.example/owned".into(),
                })),
            })
            .collect(),
    };
    assert_eq!(response.validate_for(&request), Ok(()));
    let initial_length = serde_json::to_vec(&response).unwrap().len();
    assert!(initial_length < MAXIMUM);
    let mut remaining = MAXIMUM - initial_length;
    for recording in &mut response.items {
        for creator in &mut recording.attribution.as_mut().unwrap().creators {
            let additional = remaining.min(2048 - creator.len());
            creator.push_str(&"x".repeat(additional));
            remaining -= additional;
        }
    }
    assert_eq!(remaining, 0);
    let exact = serde_json::to_vec(&response).unwrap();
    assert_eq!(exact.len(), MAXIMUM);
    assert_eq!(response.validate_for(&request), Ok(()));
    let decoded: BackingTracksResponse = serde_json::from_slice(&exact).unwrap();
    assert_eq!(decoded.validate_for(&request), Ok(()));
    assert_eq!(serde_json::to_vec(&decoded).unwrap(), exact);

    let final_creator = response
        .items
        .last_mut()
        .unwrap()
        .attribution
        .as_mut()
        .unwrap()
        .creators
        .last_mut()
        .unwrap();
    assert!(final_creator.len() < 2048);
    final_creator.push('x');
    assert_eq!(serde_json::to_vec(&response).unwrap().len(), MAXIMUM + 1);
    response.recording.validate().unwrap();
    assert_eq!(response.items.len(), 16);
    for recording in &response.items {
        recording.validate().unwrap();
        assert_ne!(recording.id, request.recording.id);
        assert!(
            recording
                .attribution
                .as_ref()
                .unwrap()
                .creators
                .iter()
                .all(|creator| {
                    creator.contains('é') && creator.contains('"') && creator.contains('\\')
                })
        );
    }
    assert_eq!(
        response.validate_for(&request),
        Err("The provider backing tracks response exceeds the size limit")
    );
}
