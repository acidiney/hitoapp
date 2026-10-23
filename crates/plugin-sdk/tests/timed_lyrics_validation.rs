use hito_plugin_sdk::{
    LyricsRequest, LyricsResponse, ProviderRecordingRef, TimedLyricLine, TimedLyrics,
};
use serde_json::json;

#[test]
fn timed_lyrics_use_explicit_bounded_intervals_without_inventing_a_duration() {
    const SEVEN_DAYS_MS: u64 = 604_800_000;
    const INVALID_INTERVAL: &str =
        "Timed lyric lines must use ordered nonoverlapping intervals within seven days";
    let request = LyricsRequest {
        recording: ProviderRecordingRef {
            id: "owned:recording".into(),
            revision: Some("owned:revision".into()),
        },
    };

    for (intervals, duration_ms) in [
        (vec![(0, 1000), (1000, 2000), (3000, 4000)], None),
        (vec![(1000, 2000)], Some(2000)),
        (vec![(SEVEN_DAYS_MS - 1, SEVEN_DAYS_MS)], None),
        (
            vec![(SEVEN_DAYS_MS - 1, SEVEN_DAYS_MS)],
            Some(SEVEN_DAYS_MS),
        ),
        ((0..2000).map(|start| (start, start + 1)).collect(), None),
    ] {
        let response = lyrics_response(&request, intervals, duration_ms);
        assert_eq!(response.validate_for(&request), Ok(()));
        let wire = serde_json::to_value(&response).unwrap();
        assert_eq!(wire["lyrics"]["duration_ms"], json!(duration_ms));
        let decoded: LyricsResponse = serde_json::from_value(wire).unwrap();
        assert_eq!(decoded.validate_for(&request), Ok(()));
        assert_eq!(decoded.lyrics.unwrap().duration_ms, duration_ms);
    }

    for intervals in [vec![], (0..2001).map(|start| (start, start + 1)).collect()] {
        let response = lyrics_response(&request, intervals, None);
        assert_eq!(
            response.validate_for(&request),
            Err("The provider must return between one and two thousand timed lyric lines")
        );
    }

    for intervals in [
        vec![(0, 0)],
        vec![(2000, 1000)],
        vec![(0, 2000), (1999, 3000)],
        vec![(2000, 3000), (0, 1000)],
        vec![(SEVEN_DAYS_MS, SEVEN_DAYS_MS + 1)],
        vec![(0, u64::MAX)],
    ] {
        let response = lyrics_response(&request, intervals, None);
        assert_eq!(response.validate_for(&request), Err(INVALID_INTERVAL));
    }

    for duration_ms in [0, 999] {
        let response = lyrics_response(&request, vec![(0, 1000)], Some(duration_ms));
        assert_eq!(
            response.validate_for(&request),
            Err("Timed lyric lines must end within the known rendition duration")
        );
    }
    for duration_ms in [SEVEN_DAYS_MS + 1, u64::MAX] {
        let response = lyrics_response(&request, vec![(0, 1000)], Some(duration_ms));
        assert_eq!(
            response.validate_for(&request),
            Err("The provider returned an unsupported lyrics duration")
        );
    }

    let valid = serde_json::to_value(lyrics_response(&request, vec![(0, 1000)], None)).unwrap();
    for number in ["-1", "0.5", "18446744073709551616"] {
        for field in ["start_ms", "end_ms", "duration_ms"] {
            let mut malformed = valid.clone();
            let number = serde_json::from_str(number).unwrap();
            if field == "duration_ms" {
                malformed["lyrics"][field] = number;
            } else {
                malformed["lyrics"]["lines"][0][field] = number;
            }
            assert!(serde_json::from_value::<LyricsResponse>(malformed).is_err());
        }
    }
}

fn lyrics_response(
    request: &LyricsRequest,
    intervals: Vec<(u64, u64)>,
    duration_ms: Option<u64>,
) -> LyricsResponse {
    LyricsResponse {
        recording: request.recording.clone(),
        lyrics: Some(TimedLyrics {
            id: "owned:lyrics".into(),
            rendition_id: "owned:rendition".into(),
            revision: None,
            rendition_label: None,
            language: None,
            duration_ms,
            attribution: None,
            lines: intervals
                .into_iter()
                .map(|(start_ms, end_ms)| TimedLyricLine {
                    start_ms,
                    end_ms,
                    text: "Owned lyric line".into(),
                })
                .collect(),
        }),
    }
}
