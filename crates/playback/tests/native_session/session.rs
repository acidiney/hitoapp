#![cfg(feature = "device")]
use hito_audio::test_support::capture;
use hito_core::test_support::playback as local_fixture;
use hito_core::test_support::provider_audio as audio_fixture;
use hito_playback::{Command, Session, Status};
use std::time::Duration;

#[tokio::test]
#[ignore = "requires the isolated clocked audio monitor"]
async fn logout_stops_and_joins_actual_output_without_another_player_command() {
    let f = local_fixture::LocalPlayback::start().await;
    let grant = f
        .services
        .playback
        .playback_source(&f.actor, &f.recording, &f.source)
        .await
        .unwrap();
    let recording = capture::Capture::open();
    let session = Session::open(grant).await.unwrap();
    session.command(Command::Resume).await.unwrap();
    tokio::time::timeout(Duration::from_secs(3), async {
        while session.snapshot().position < Duration::from_millis(50) {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    f.services.identity.logout(&f.token).await.unwrap();
    tokio::time::timeout(Duration::from_secs(2), async {
        while session.snapshot().status != Status::Revoked {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("authorization withdrawal must stop and join output");
    assert!(!session.snapshot().playing);
    assert!(session.command(Command::Resume).await.is_err());
    recording.after(recording.mark(), 0.3);
    assert!(capture::energy(&recording.recent(0.1)) < 0.000001);
    session.close().await.unwrap();
}

#[tokio::test]
#[ignore = "requires the isolated clocked audio monitor"]
async fn cancelling_the_close_waiter_still_reclaims_an_owner_blocked_during_startup() {
    let f = audio_fixture::AudioLibrary::start().await;
    f.ready().await;
    let gate = f.server.hold("audio-tail").await;
    let grant = f
        .hito
        .playback
        .playback_source(&f.actor, &f.source.recording_id, &f.source.id)
        .await
        .unwrap();
    let session = Session::start(grant);
    let mut observed = session.subscribe();
    gate.entered().await;
    tokio::time::timeout(
        Duration::from_secs(2),
        observed.wait_for(|state| state.status == Status::Starting),
    )
    .await
    .unwrap()
    .unwrap();
    let closing = tokio::spawn(session.close());
    tokio::task::yield_now().await;
    closing.abort();
    tokio::time::timeout(
        Duration::from_secs(2),
        observed.wait_for(|state| state.status == Status::Stopped),
    )
    .await
    .expect("the owner must join after its observer disappears")
    .unwrap();
    let first = f
        .hito
        .playback
        .open_audio(&f.actor, &f.source.id, Default::default())
        .await
        .unwrap();
    let second = f
        .hito
        .playback
        .open_audio(&f.actor, &f.source.id, Default::default())
        .await
        .unwrap();
    drop((first, second));
    gate.release();
}

#[tokio::test]
#[ignore = "requires the isolated clocked audio monitor"]
async fn a_queue_owner_can_stop_pending_startup_before_waiting_for_cleanup() {
    let f = audio_fixture::AudioLibrary::start().await;
    f.ready().await;
    let gate = f.server.hold("audio-tail").await;
    let grant = f
        .hito
        .playback
        .playback_source(&f.actor, &f.source.recording_id, &f.source.id)
        .await
        .unwrap();
    let session = Session::start(grant);
    let mut observed = session.subscribe();
    gate.entered().await;
    session.stop();
    tokio::time::timeout(
        Duration::from_secs(2),
        observed.wait_for(|state| state.status == Status::Stopped),
    )
    .await
    .expect("stop must cancel startup independently of a close waiter")
    .unwrap();
    assert!(session.ready().await.is_err());
    session.close().await.unwrap();
    gate.release();
}

#[tokio::test]
#[ignore = "requires the isolated clocked audio monitor"]
async fn readiness_cannot_succeed_after_a_ready_session_has_been_stopped() {
    let f = local_fixture::LocalPlayback::start().await;
    let grant = f
        .services
        .playback
        .playback_source(&f.actor, &f.recording, &f.source)
        .await
        .unwrap();
    let session = Session::open(grant).await.unwrap();
    session.stop();
    assert!(
        session.ready().await.is_err(),
        "stop must immediately fence readiness observers"
    );
    session.close().await.unwrap();
}
