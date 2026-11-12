#![cfg(feature = "device")]
use hito_audio::test_support::capture;
use hito_core::test_support::playback as fixture;
use hito_core::test_support::support;
use hito_playback::{Player, PlayerAction, PlayerCommand};
use std::time::Duration;
use uuid::Uuid;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires the isolated clocked audio monitor"]
async fn a_missing_first_copy_falls_back_to_another_verified_source_of_the_recording() {
    let f = fixture::LocalPlayback::start().await;
    std::fs::write(f.music.join("copy.wav"), &f.original).unwrap();
    support::scan(&f.services, &f.actor, &f.library).await;
    let sources = f
        .services
        .catalog
        .sources(&f.actor, &f.recording)
        .await
        .unwrap();
    assert_eq!(sources.len(), 2);
    let first = if sources[0].id == f.source {
        "track.wav"
    } else {
        "copy.wav"
    };
    std::fs::remove_file(f.music.join(first)).unwrap();
    let capture = capture::Capture::open();
    let diagnostics = hito_testkit::logs::LogCapture::unbounded();
    let dispatch = if std::env::var_os("HITO_CAPTURE_DUMP").is_some() {
        diagnostics.dispatch("hito_playback=debug,hito_audio=debug")
    } else {
        tracing::Dispatch::none()
    };
    let (player, owner) =
        tracing::dispatcher::with_default(&dispatch, || Player::native(f.services.clone()));
    let attached = player
        .claim(&f.actor, player.inspect(&f.actor).await.unwrap().epoch)
        .await
        .unwrap();
    let queued = player
        .submit(
            &f.actor,
            PlayerCommand {
                id: Uuid::new_v4(),
                lease: attached.lease,
                sequence: 1,
                revision: 0,
                action: PlayerAction::Replace {
                    recordings: vec![Uuid::parse_str(&f.recording).unwrap()],
                    start: 0,
                },
            },
        )
        .await
        .unwrap();
    player
        .submit(
            &f.actor,
            PlayerCommand {
                id: Uuid::new_v4(),
                lease: attached.lease,
                sequence: 2,
                revision: queued.revision,
                action: PlayerAction::Resume {},
            },
        )
        .await
        .unwrap();
    let mut position = player
        .subscribe_position(&f.actor, attached.lease)
        .await
        .unwrap();
    tokio::time::timeout(
        Duration::from_secs(3),
        position.wait_for(|state| state.position_ms > 100),
    )
    .await
    .unwrap()
    .unwrap();
    let state = player.inspect(&f.actor).await.unwrap().state.unwrap();
    assert_eq!(state.selection, 1);
    assert_eq!(state.current.unwrap().recording_id.to_string(), f.recording);
    capture.after(capture.mark(), 0.1);
    let energy = capture::energy(&capture.recent(0.05));
    if std::env::var_os("HITO_CAPTURE_DUMP").is_some() {
        eprintln!(
            "observed={:?} state={:?} capture_frames={} energy={energy}\n{}",
            *position.borrow(),
            state,
            capture.mark(),
            diagnostics.text()
        );
        hito_audio::test_support::continuity::dump("recovery", &capture.all());
    }
    assert!(energy > 0.00001);
    owner.close().await.unwrap();
}
