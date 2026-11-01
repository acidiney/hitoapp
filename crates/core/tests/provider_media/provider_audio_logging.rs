use audio_fixture::AudioLibrary;
use axum::http::StatusCode;
use hito_core::OpenedAudio;
use hito_core::test_support::metadata_tag_logs::CapturedLogs;
use hito_core::test_support::provider_audio as audio_fixture;
use serde_json::Value;
use tracing::instrument::WithSubscriber;

#[tokio::test]
async fn a_provider_stream_that_ends_early_logs_one_safe_warning_and_no_error() {
    let logs = CapturedLogs::default();
    async {
        let f = AudioLibrary::start().await;
        let mut headers = audio_fixture::headers();
        headers.insert(
            "content-length",
            (f.original.len() + 4).to_string().parse().unwrap(),
        );
        f.server
            .audio(StatusCode::OK, headers, f.original.clone())
            .await;
        let gate = f.server.hold("audio-body").await;
        let OpenedAudio::Content(mut audio) = f
            .hito
            .playback
            .open_audio(&f.actor, &f.source.id, Default::default())
            .await
            .unwrap()
        else {
            panic!("audio headers must open")
        };
        let mut read = 0;
        while read < f.original.len() {
            read += audio
                .body
                .as_mut()
                .unwrap()
                .chunk()
                .await
                .unwrap()
                .unwrap()
                .len();
        }
        gate.release();
        assert!(audio.body.as_mut().unwrap().chunk().await.is_err());
    }
    .with_subscriber(logs.subscriber())
    .await;
    let output = logs.text();
    let loud: Vec<Value> = output
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .filter(|event| matches!(event["level"].as_str(), Some("ERROR" | "WARN")))
        .collect();
    assert_eq!(loud.len(), 1, "one failed read must log once: {loud:#?}");
    assert_eq!(loud[0]["level"], "WARN");
    assert_eq!(loud[0]["fields"]["event"], "audio_stream_failed");
    assert_eq!(loud[0]["fields"]["error_kind"], "upstream");
    assert!(!output.contains("private-audio-token"));
}
