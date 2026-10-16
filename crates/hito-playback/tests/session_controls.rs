#![cfg(feature = "device")]
#[path = "../../hito-core/tests/fixtures/provider_audio.rs"]
mod audio_fixture;
#[path = "../../hito-audio/tests/support/capture.rs"]
#[allow(dead_code)]
mod capture;
#[path = "../../hito-core/tests/fixtures/content_service.rs"]
mod fixture;
#[path = "../../hito-core/tests/support/mod.rs"]
mod support;
use hito_playback::{Command, Session, Status};
use std::time::Duration;

#[tokio::test]
#[ignore = "requires the isolated clocked audio monitor"]
async fn provider_output_obeys_controls_and_is_reclaimed_when_permission_changes_while_paused() {
    let f = audio_fixture::AudioLibrary::start().await;
    f.ready().await;
    f.server.range_chunks(1024).await;
    let grant = f
        .hito
        .catalog
        .playback_source(&f.actor, &f.source.recording_id, &f.source.id)
        .await
        .unwrap();
    let recording = capture::Capture::open();
    let session = Session::open(grant).await.unwrap();
    assert!(!session.snapshot().playing);
    session.command(Command::Resume).await.unwrap();
    tokio::time::timeout(Duration::from_secs(3), async {
        while session.snapshot().position < Duration::from_millis(100) {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    recording.after(recording.mark(), 0.1);
    assert!(capture::energy(&recording.recent(0.05)) > 0.00001);
    session.command(Command::Pause).await.unwrap();
    session
        .command(Command::Seek(Duration::from_millis(600)))
        .await
        .unwrap();
    session.command(Command::Volume(0.25)).await.unwrap();
    assert!(session.command(Command::Volume(f32::NAN)).await.is_err());
    assert!(!session.snapshot().playing);
    assert_eq!(session.snapshot().position, Duration::from_millis(600));
    f.hito
        .plugins
        .withdraw_service(&f.actor, &f.installation.service_id, 1)
        .await
        .unwrap();
    let mut state = session.subscribe();
    tokio::time::timeout(
        Duration::from_secs(2),
        state.wait_for(|state| state.status == Status::Revoked),
    )
    .await
    .unwrap()
    .unwrap();
    assert!(session.command(Command::Resume).await.is_err());
    session.close().await.unwrap();
}
