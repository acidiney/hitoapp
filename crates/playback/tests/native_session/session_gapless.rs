#![cfg(feature = "device")]
#[path = "../fixtures/two_tracks.rs"]
mod fixture;
use hito_audio::test_support::capture::{Capture, energy};
use hito_audio::test_support::continuity::{audit, counter, heard};
use hito_playback::{Command, Error, Session, Snapshot, Status};
use std::time::Duration;

async fn until(session: &Session, condition: impl Fn(&Snapshot) -> bool) {
    tokio::time::timeout(Duration::from_secs(5), async {
        while !condition(&session.snapshot()) {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("session condition timed out");
}

#[tokio::test]
#[ignore = "requires the isolated clocked audio monitor"]
async fn a_prepared_next_input_continues_in_the_same_stream_without_silence() {
    let f = fixture::TwoTracks::start(24_000, 36_000).await;
    let capture = Capture::open();
    let session = Session::open(f.grant(&f.first).await).await.unwrap();
    session
        .prepare_next(f.grant(&f.second).await)
        .await
        .unwrap();
    session.command(Command::Resume).await.unwrap();
    until(&session, |state| state.ended).await;
    let ended = session.snapshot();
    assert_eq!(ended.track, 1);
    assert_eq!(
        ended.format.and_then(|format| format.duration),
        Some(Duration::from_millis(750))
    );
    session.close().await.unwrap();
    capture.after(capture.mark(), 0.3);
    let audit = audit(&capture.all(), counter, 60_000);
    eprintln!("session gap frames={}", audit.longest_silence);
    assert_eq!(audit.longest_silence, 0);
    assert_eq!(audit.mismatches, 0);
    assert_eq!(audit.heard, 60_000);
}

#[tokio::test]
#[ignore = "requires the isolated clocked audio monitor"]
async fn a_withdrawn_next_input_never_plays() {
    let f = fixture::TwoTracks::start(24_000, 36_000).await;
    let capture = Capture::open();
    let session = Session::open(f.grant(&f.first).await).await.unwrap();
    session
        .prepare_next(f.grant(&f.second).await)
        .await
        .unwrap();
    assert!(session.cancel_next().await.unwrap());
    assert!(!session.cancel_next().await.unwrap());
    session.command(Command::Resume).await.unwrap();
    until(&session, |state| state.ended).await;
    assert_eq!(session.snapshot().track, 0);
    session.close().await.unwrap();
    capture.after(capture.mark(), 0.3);
    assert_eq!(heard(&capture.all()).len(), 24_000);
}

#[tokio::test]
#[ignore = "requires the isolated clocked audio monitor"]
async fn a_next_input_offered_after_the_stream_ended_is_refused() {
    let f = fixture::TwoTracks::start(2_400, 36_000).await;
    let session = Session::open(f.grant(&f.first).await).await.unwrap();
    session.command(Command::Resume).await.unwrap();
    until(&session, |state| state.ended).await;
    let refused = session.prepare_next(f.grant(&f.second).await).await;
    assert_eq!(refused, Err(Error::Native(hito_audio::Error::Interrupted)));
    assert_eq!(session.snapshot().track, 0);
    session.close().await.unwrap();
}

#[tokio::test]
#[ignore = "requires the isolated clocked audio monitor"]
async fn logout_after_the_boundary_still_stops_output_through_the_new_source() {
    let f = fixture::TwoTracks::start(12_000, 480_000).await;
    let capture = Capture::open();
    let session = Session::open(f.grant(&f.first).await).await.unwrap();
    session
        .prepare_next(f.grant(&f.second).await)
        .await
        .unwrap();
    session.command(Command::Resume).await.unwrap();
    until(&session, |state| {
        state.track == 1 && state.position > Duration::from_millis(100)
    })
    .await;
    f.services.identity.logout(&f.token).await.unwrap();
    until(&session, |state| state.status == Status::Revoked).await;
    assert!(!session.snapshot().playing);
    capture.after(capture.mark(), 0.3);
    assert!(energy(&capture.recent(0.1)) < 0.000001);
    session.close().await.unwrap();
}

#[tokio::test]
#[ignore = "requires the isolated clocked audio monitor"]
async fn withdrawing_only_the_new_source_after_the_boundary_stops_output() {
    let f = fixture::TwoTracks::start(12_000, 480_000).await;
    let capture = Capture::open();
    let session = Session::open(f.grant(&f.first).await).await.unwrap();
    session
        .prepare_next(f.grant(&f.second).await)
        .await
        .unwrap();
    session.command(Command::Resume).await.unwrap();
    until(&session, |state| {
        state.track == 1 && state.position > Duration::from_millis(100)
    })
    .await;
    std::fs::remove_file(f.music.join("second.wav")).unwrap();
    hito_core::test_support::support::scan(&f.services, &f.actor, &f.library).await;
    until(&session, |state| state.status == Status::Revoked).await;
    assert!(!session.snapshot().playing);
    capture.after(capture.mark(), 0.3);
    assert!(energy(&capture.recent(0.1)) < 0.000001);
    session.close().await.unwrap();
}
