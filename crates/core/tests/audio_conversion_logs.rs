use hito_testkit::tone_files as tones;

use hito_core::test_support::metadata_tag_logs::CapturedLogs;
use hito_core::test_support::support;
use hito_core::{AudioRequest, QualityAudio, QualityRequest, Search, StreamQuality};
use std::time::Duration;

#[tokio::test]
async fn conversion_logs_outcomes_without_private_names_or_paths() {
    let logs = CapturedLogs::default();
    tracing::subscriber::set_global_default(logs.subscriber()).unwrap();
    let (directory, services, actor, library, music) = support::library("conversion-logs").await;
    tones::flac(&music.join("Private Secret Title.flac"), 5);
    support::scan(&services, &actor, &library).await;
    let recording = services
        .catalog
        .recordings(&actor, Search::default())
        .await
        .unwrap()
        .items
        .remove(0)
        .id;
    let source = services
        .catalog
        .sources(&actor, &recording)
        .await
        .unwrap()
        .remove(0)
        .id;
    let QualityAudio::Converted(mut converted) = services
        .playback
        .open_quality_audio(
            &actor,
            &source,
            QualityRequest {
                quality: StreamQuality::DataSaver,
                start: Duration::from_secs(1),
                original: AudioRequest::default(),
            },
        )
        .await
        .unwrap()
    else {
        panic!("the source must be converted");
    };
    let mut body = converted.body.take().unwrap();
    while body.chunk().await.unwrap().is_some() {}
    drop(body);
    let text = logs.text();
    assert!(text.contains("Audio conversion started"), "{text}");
    assert!(text.contains("Audio conversion closed"), "{text}");
    assert!(text.contains("complete"), "{text}");
    for private in [
        "Private",
        "Secret",
        "track",
        directory.path().to_str().unwrap(),
        "/dev/stdin",
        "pipe:1",
    ] {
        assert!(
            !text.contains(private),
            "{private} leaked into logs: {text}"
        );
    }
}
