#![cfg(feature = "device")]
use hito_core::test_support::playback as fixture;
use hito_playback::{
    MediaCommand, MediaIntegration, MediaPlayback, MediaSink, MediaTrack, Player, PlayerAction,
    PlayerCommand, PlayerPhase, media_commands,
};
use std::time::Duration;
use tokio::sync::mpsc;
use uuid::Uuid;

enum Event {
    Track,
    Playback(MediaPlayback),
    Clear,
}
struct Recorder(mpsc::UnboundedSender<Event>);
impl MediaSink for Recorder {
    fn set_track(&mut self, _: &MediaTrack) {
        let _ = self.0.send(Event::Track);
    }
    fn set_playback(&mut self, playback: MediaPlayback) {
        let _ = self.0.send(Event::Playback(playback));
    }
    fn clear(&mut self) {
        let _ = self.0.send(Event::Clear);
    }
}
async fn playback(
    events: &mut mpsc::UnboundedReceiver<Event>,
    wanted: impl Fn(&MediaPlayback) -> bool,
) -> MediaPlayback {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let Event::Playback(state) = events.recv().await.unwrap()
                && wanted(&state)
            {
                return state;
            }
        }
    })
    .await
    .expect("expected system media playback state")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires the isolated clocked audio monitor"]
async fn system_commands_drive_real_native_playback_and_publish_its_state() {
    let f = fixture::LocalPlayback::start().await;
    let (events, mut received) = mpsc::unbounded_channel();
    let (sender, commands) = media_commands();
    let (player, owner) = Player::native_with_media(
        f.services.clone(),
        MediaIntegration::new(Recorder(events), commands),
    );
    let lease = player
        .claim(&f.actor, player.inspect(&f.actor).await.unwrap().epoch)
        .await
        .unwrap()
        .lease;
    player
        .submit(
            &f.actor,
            PlayerCommand {
                id: Uuid::new_v4(),
                lease,
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
    let mut state = player.subscribe(&f.actor, lease).await.unwrap();
    tokio::time::timeout(
        Duration::from_secs(3),
        state.wait_for(|state| state.phase == PlayerPhase::Ready),
    )
    .await
    .unwrap()
    .unwrap();
    assert!(matches!(received.recv().await, Some(Event::Track)));

    sender.try_send(MediaCommand::Play).unwrap();
    playback(&mut received, |s| {
        matches!(s, MediaPlayback::Playing { .. })
    })
    .await;
    assert!(state.borrow().desired_playing);

    sender.try_send(MediaCommand::Pause).unwrap();
    playback(&mut received, |s| matches!(s, MediaPlayback::Paused { .. })).await;
    assert!(!state.borrow().desired_playing);

    sender
        .try_send(MediaCommand::Seek { position_ms: 400 })
        .unwrap();
    let seeked = playback(&mut received, |s| {
        matches!(s, MediaPlayback::Paused { position_ms: 400 })
    })
    .await;
    assert_eq!(seeked, MediaPlayback::Paused { position_ms: 400 });

    sender.try_send(MediaCommand::Toggle).unwrap();
    playback(&mut received, |s| {
        matches!(s, MediaPlayback::Playing { .. })
    })
    .await;

    sender.try_send(MediaCommand::Stop).unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        while !matches!(received.recv().await, Some(Event::Clear)) {}
    })
    .await
    .expect("stop clears system media");
    owner.close().await.unwrap();
}
