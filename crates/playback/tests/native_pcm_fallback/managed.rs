use super::fixture::until;
use hito_audio::test_support::capture::{Capture, energy};
use hito_core::{DownloadRequest, DownloadState, test_support::provider_audio::AudioLibrary};
use hito_playback::{Command, Session};
use std::{future::pending, process::Command as Tool, time::Duration};

#[tokio::test]
#[ignore = "requires the isolated clocked audio monitor"]
async fn explicitly_saved_opus_plays_natively_with_the_provider_offline_and_retains_its_intent() {
    let f = AudioLibrary::start().await;
    let path = f.music.join("owned.opus");
    assert!(
        Tool::new("ffmpeg")
            .args([
                "-v",
                "error",
                "-y",
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=440:sample_rate=48000:duration=2",
                "-c:a",
                "libopus"
            ])
            .arg(&path)
            .status()
            .unwrap()
            .success()
    );
    let original = std::fs::read(&path).unwrap();
    f.server
        .audio(
            axum::http::StatusCode::OK,
            axum::http::HeaderMap::from_iter([(
                "content-type".parse().unwrap(),
                "audio/ogg".parse().unwrap(),
            )]),
            original.clone(),
        )
        .await;
    let receipt = f
        .hito
        .downloads
        .create(
            &f.actor,
            &f.source.recording_id,
            DownloadRequest {
                request_id: uuid::Uuid::new_v4(),
            },
        )
        .await
        .unwrap();
    assert!(
        f.hito
            .downloads
            .run_next_job_until(pending())
            .await
            .unwrap()
    );
    let saved = f
        .hito
        .downloads
        .status(&f.actor, &f.source.recording_id)
        .await
        .unwrap();
    assert!(matches!(saved.status, DownloadState::Ready));
    f.server.offline().await;
    let source = saved.source_id.as_deref().unwrap();
    let grant = f
        .hito
        .playback
        .playback_source(&f.actor, &f.source.recording_id, source)
        .await
        .unwrap();
    assert!(grant.is_local());
    let capture = Capture::open();
    let session = Session::open(grant.clone()).await.unwrap();
    session.command(Command::Resume).await.unwrap();
    until(&session, |state| {
        state.position > Duration::from_millis(150)
    })
    .await;
    capture.after(capture.mark(), 0.1);
    assert!(energy(&capture.recent(0.1)) > 0.0001);
    session.close().await.unwrap();
    let after = f
        .hito
        .downloads
        .status(&f.actor, &f.source.recording_id)
        .await
        .unwrap();
    assert!(matches!(after.status, DownloadState::Ready));
    assert_eq!(after.intent_id, receipt.intent_id);
    assert_eq!(after.source_id.as_deref(), Some(source));
    assert_eq!(std::fs::read(path).unwrap(), original);
    let mut retained = grant.open_file().await.unwrap().file;
    let mut bytes = Vec::new();
    std::io::Read::read_to_end(&mut retained, &mut bytes).unwrap();
    assert_eq!(bytes, original);
}
