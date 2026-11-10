use hito_core::{
    DownloadRequest, DownloadState, Error, test_support::provider_audio::AudioLibrary,
};
use std::{future::pending, time::Duration};

#[tokio::test]
async fn saved_opus_decodes_offline_without_changing_download_intent_or_original_audio() {
    let library = AudioLibrary::start().await;
    let original = library.music.join("owned.opus");
    assert!(
        std::process::Command::new("ffmpeg")
            .args([
                "-v",
                "error",
                "-y",
                "-f",
                "lavfi",
                "-i",
                "aevalsrc=0.1*sin(2*PI*440*t)|0.1*sin(2*PI*880*t):s=48000:d=2",
                "-c:a",
                "libopus",
                "-ac",
                "2"
            ])
            .arg(&original)
            .status()
            .unwrap()
            .success()
    );
    let bytes = std::fs::read(&original).unwrap();
    library
        .server
        .audio(
            axum::http::StatusCode::OK,
            axum::http::HeaderMap::from_iter([(
                "content-type".parse().unwrap(),
                "audio/ogg".parse().unwrap(),
            )]),
            bytes.clone(),
        )
        .await;
    let provider = library
        .hito
        .playback
        .playback_source(
            &library.actor,
            &library.source.recording_id,
            &library.source.id,
        )
        .await
        .unwrap();
    assert!(matches!(
        provider.open_pcm(Duration::ZERO).await,
        Err(Error::UnsupportedAudio)
    ));
    let receipt = library
        .hito
        .downloads
        .create(
            &library.actor,
            &library.source.recording_id,
            DownloadRequest {
                request_id: uuid::Uuid::new_v4(),
            },
        )
        .await
        .unwrap();
    assert!(
        library
            .hito
            .downloads
            .run_next_job_until(pending())
            .await
            .unwrap()
    );
    let saved = library
        .hito
        .downloads
        .status(&library.actor, &library.source.recording_id)
        .await
        .unwrap();
    assert!(matches!(saved.status, DownloadState::Ready));
    library.server.offline().await;
    let source = saved.source_id.as_deref().unwrap();
    let grant = library
        .hito
        .playback
        .playback_source(&library.actor, &library.source.recording_id, source)
        .await
        .unwrap();
    let mut stream = grant.open_pcm(Duration::ZERO).await.unwrap();
    assert_eq!(stream.format().sample_rate, 48_000);
    assert_eq!(stream.format().channels, 2);
    let mut count = 0;
    let mut crossings = [0; 2];
    let mut previous = [0.0_f32; 2];
    while let Some(block) = stream.next_block().await.unwrap() {
        assert_eq!(
            block.position,
            Duration::from_secs_f64(count as f64 / 96_000.0)
        );
        assert_eq!(block.samples.len() % 2, 0);
        for frame in block.samples.as_chunks::<2>().0 {
            for channel in 0..2 {
                if previous[channel] <= 0.0 && frame[channel] > 0.0 {
                    crossings[channel] += 1;
                }
                previous[channel] = frame[channel];
            }
        }
        count += block.samples.len();
    }
    assert_eq!(count, 192_000);
    assert!((878..=882).contains(&crossings[0]));
    assert!((1758..=1762).contains(&crossings[1]));
    stream.close().await.unwrap();
    let after = library
        .hito
        .downloads
        .status(&library.actor, &library.source.recording_id)
        .await
        .unwrap();
    assert!(matches!(after.status, DownloadState::Ready));
    assert_eq!(after.intent_id, receipt.intent_id);
    assert_eq!(after.source_id.as_deref(), Some(source));
    assert_eq!(std::fs::read(original).unwrap(), bytes);
    let mut retained = grant.open_file().await.unwrap().file;
    use std::io::Read;
    let mut retained_bytes = Vec::new();
    retained.read_to_end(&mut retained_bytes).unwrap();
    assert_eq!(retained_bytes, bytes);
}
