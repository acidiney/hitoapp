use hito_plugin_sdk::{
    LyricsRequest, LyricsResponse, ProviderRecordingRef, RecordingAttribution, TimedLyricLine,
    TimedLyrics,
};

#[test]
fn complete_encoded_lyrics_accept_the_exact_budget_and_reject_one_more_valid_byte() {
    const MAXIMUM: usize = 262_144;
    let request = LyricsRequest {
        recording: ProviderRecordingRef {
            id: "é".repeat(128),
            revision: Some("r".repeat(256)),
        },
    };
    request.validate().unwrap();
    assert_eq!(
        LyricsResponse {
            recording: request.recording.clone(),
            lyrics: None,
        }
        .validate_for(&request),
        Ok(())
    );
    let mut response = LyricsResponse {
        recording: request.recording.clone(),
        lyrics: Some(TimedLyrics {
            id: "d".repeat(256),
            rendition_id: "t".repeat(256),
            revision: Some("v".repeat(256)),
            rendition_label: Some("é".repeat(1024)),
            language: Some("l".repeat(64)),
            duration_ms: None,
            attribution: Some(RecordingAttribution {
                creators: vec!["Owned créateur \"quoted\" \\ ensemble".into(); 16],
                provider_label: "Owned provider".into(),
                content_url: "https://lyrics.example/owned".into(),
            }),
            lines: (0..2000)
                .map(|index| TimedLyricLine {
                    start_ms: index * 1000,
                    end_ms: (index + 1) * 1000,
                    text: "Owned é \"phrase\" \\ take".into(),
                })
                .collect(),
        }),
    };
    assert_eq!(response.validate_for(&request), Ok(()));
    let initial_length = serde_json::to_vec(&response).unwrap().len();
    assert!(initial_length < MAXIMUM);
    let mut remaining = MAXIMUM - initial_length;
    for line in &mut response.lyrics.as_mut().unwrap().lines {
        let additional = remaining.min(2048 - line.text.len());
        line.text.push_str(&"x".repeat(additional));
        remaining -= additional;
    }
    assert_eq!(remaining, 0);
    let exact = serde_json::to_vec(&response).unwrap();
    assert_eq!(exact.len(), MAXIMUM);
    assert_eq!(response.validate_for(&request), Ok(()));
    let decoded: LyricsResponse = serde_json::from_slice(&exact).unwrap();
    assert_eq!(decoded.validate_for(&request), Ok(()));
    assert_eq!(serde_json::to_vec(&decoded).unwrap(), exact);

    let final_line = response.lyrics.as_mut().unwrap().lines.last_mut().unwrap();
    assert!(final_line.text.len() < 2048);
    final_line.text.push('x');
    let oversized = serde_json::to_vec(&response).unwrap();
    assert_eq!(oversized.len(), MAXIMUM + 1);
    response.recording.validate().unwrap();
    let lyrics = response.lyrics.as_ref().unwrap();
    lyrics.attribution.as_ref().unwrap().validate().unwrap();
    assert_eq!(lyrics.lines.len(), 2000);
    assert_eq!(lyrics.duration_ms, None);
    assert!(lyrics.lines.iter().all(|line| {
        line.text.len() <= 2048
            && line.text.contains('é')
            && line.text.contains('"')
            && line.text.contains('\\')
    }));
    assert!(
        lyrics
            .lines
            .iter()
            .map(|line| line.text.len())
            .sum::<usize>()
            < MAXIMUM
    );
    assert_eq!(
        response.validate_for(&request),
        Err("The provider lyrics response exceeds the size limit")
    );
}
