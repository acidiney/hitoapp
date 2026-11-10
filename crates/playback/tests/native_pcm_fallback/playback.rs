use super::fixture::{Library, open_with_diagnostics, until};
use hito_audio::test_support::capture::{Capture, energy};
use hito_playback::{Command, Session};
use std::time::Duration;

#[tokio::test]
#[ignore = "requires the isolated clocked audio monitor"]
async fn owned_opus_plays_through_the_authorized_native_session_without_changing_files_or_downloads()
 {
    let f = Library::start().await;
    assert!(matches!(
        f.services.downloads.status(&f.actor, &f.recording).await,
        Err(hito_core::Error::NotFound)
    ));
    let capture = Capture::open();
    let session = open_with_diagnostics(f.grant().await).await;
    session.command(Command::Resume).await.unwrap();
    until(&session, |state| {
        state.position > Duration::from_millis(150)
    })
    .await;
    capture.after(capture.mark(), 0.1);
    let heard = capture.recent(0.1);
    assert!(energy(&heard) > 0.0001);
    let crossings = heard
        .windows(2)
        .filter(|window| window[0] <= 0. && window[1] > 0.)
        .count();
    assert!(
        (42..=46).contains(&crossings),
        "expected 440Hz output, heard {crossings} crossings in 100ms"
    );
    let format = session.snapshot().format.unwrap();
    assert_eq!(format.sample_rate, 48_000);
    assert_eq!(format.channels, 1);
    assert!((format.duration.unwrap().as_secs_f64() - 4.).abs() < 0.02);
    session.close().await.unwrap();
    assert_eq!(std::fs::read(&f.path).unwrap(), f.original);
    assert!(matches!(
        f.services.downloads.status(&f.actor, &f.recording).await,
        Err(hito_core::Error::NotFound)
    ));
}

#[tokio::test]
#[ignore = "requires the isolated clocked audio monitor"]
async fn pcm_seeks_pause_speed_completion_and_restart_keep_the_source_timeline() {
    let f = Library::start().await;
    let capture = Capture::open();
    let session = Session::open(f.grant().await).await.unwrap();
    for position in [
        Duration::from_millis(2_500),
        Duration::from_millis(250),
        Duration::ZERO,
    ] {
        session.command(Command::Seek(position)).await.unwrap();
        assert_eq!(session.snapshot().position, position);
        assert!(!session.snapshot().playing);
        assert!(
            (session
                .snapshot()
                .format
                .unwrap()
                .duration
                .unwrap()
                .as_secs_f64()
                - 4.)
                .abs()
                < 0.02
        );
    }
    session.command(Command::Resume).await.unwrap();
    until(&session, |state| {
        state.position > Duration::from_millis(100)
    })
    .await;
    capture.after(capture.mark(), 0.1);
    assert!(energy(&capture.recent(0.1)) > 0.0001);
    session.command(Command::Pause).await.unwrap();
    session
        .command(Command::Seek(Duration::from_millis(250)))
        .await
        .unwrap();
    let paused = session.snapshot().position;
    tokio::time::sleep(Duration::from_millis(120)).await;
    assert_eq!(session.snapshot().position, paused);
    session.command(Command::Speed(2.)).await.unwrap();
    session
        .command(Command::Seek(Duration::from_millis(3_500)))
        .await
        .unwrap();
    session.command(Command::Resume).await.unwrap();
    until(&session, |state| state.ended).await;
    assert!((session.snapshot().position.as_secs_f64() - 4.).abs() < 0.03);
    assert_eq!(session.snapshot().speed, 2_000);
    session.command(Command::Resume).await.unwrap();
    until(&session, |state| {
        state.playing
            && !state.ended
            && state.position > Duration::from_millis(100)
            && state.position < Duration::from_secs(1)
    })
    .await;
    session.close().await.unwrap();
    assert_eq!(std::fs::read(&f.path).unwrap(), f.original);
}
