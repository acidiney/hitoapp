use hito_core::{Search, test_support::playback as fixture};
use hito_playback::{
    MediaCommand, MediaIntegration, MediaPlayback, MediaSink, MediaTrack, Player, PlayerAction,
    PlayerCommand, media_commands,
};
use std::time::Duration;
use tokio::sync::mpsc;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq)]
enum Event {
    Track(MediaTrack),
    Playback(MediaPlayback),
    Clear,
}
struct Recorder(mpsc::UnboundedSender<Event>);
impl MediaSink for Recorder {
    fn set_track(&mut self, track: &MediaTrack) {
        let _ = self.0.send(Event::Track(track.clone()));
    }
    fn set_playback(&mut self, playback: MediaPlayback) {
        let _ = self.0.send(Event::Playback(playback));
    }
    fn clear(&mut self) {
        let _ = self.0.send(Event::Clear);
    }
}
async fn next(events: &mut mpsc::UnboundedReceiver<Event>) -> Event {
    tokio::time::timeout(Duration::from_secs(5), events.recv())
        .await
        .expect("a system media event")
        .unwrap()
}
async fn quiet(events: &mut mpsc::UnboundedReceiver<Event>) {
    assert!(
        tokio::time::timeout(Duration::from_millis(400), events.recv())
            .await
            .is_err(),
        "no further system media event"
    );
}
fn png(width: u32, height: u32) -> Vec<u8> {
    let mut bytes = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
    bytes.extend(width.to_be_bytes());
    bytes.extend(height.to_be_bytes());
    bytes.extend([8, 6, 0, 0, 0]);
    bytes
}
fn command(lease: Uuid, sequence: u64, revision: u64, action: PlayerAction) -> PlayerCommand {
    PlayerCommand {
        id: Uuid::new_v4(),
        lease,
        sequence,
        revision,
        action,
    }
}
fn start(
    f: &fixture::LocalPlayback,
) -> (
    Player,
    hito_playback::PlayerOwner,
    mpsc::UnboundedReceiver<Event>,
    hito_playback::MediaCommandSender,
) {
    let (events, received) = mpsc::unbounded_channel();
    let (sender, commands) = media_commands();
    let (player, owner) = Player::start_with_media(
        f.services.clone(),
        MediaIntegration::new(Recorder(events), commands),
    );
    (player, owner, received, sender)
}

#[tokio::test]
async fn authorized_metadata_publishes_once_for_a_selected_track_and_system_stop_clears_it() {
    let f = fixture::LocalPlayback::with_audio(|path| {
        std::fs::write(path.parent().unwrap().join("cover.png"), png(600, 600)).unwrap();
        hito_core::test_support::support::write_wav(path, 0)
    })
    .await;
    let recording = f
        .services
        .catalog
        .recordings(&f.actor, Search::default())
        .await
        .unwrap()
        .items
        .remove(0);
    let (player, owner, mut events, sender) = start(&f);
    let lease = player
        .claim(&f.actor, player.inspect(&f.actor).await.unwrap().epoch)
        .await
        .unwrap()
        .lease;
    let id = Uuid::parse_str(&f.recording).unwrap();
    player
        .submit(
            &f.actor,
            command(
                lease,
                1,
                0,
                PlayerAction::Replace {
                    recordings: vec![id, id],
                    start: 0,
                },
            ),
        )
        .await
        .unwrap();
    let Event::Track(track) = next(&mut events).await else {
        panic!("metadata comes first");
    };
    assert_eq!(track.title, recording.title);
    assert_eq!(track.artist, recording.artist);
    assert_eq!(track.album, recording.album.clone().unwrap_or_default());
    assert_eq!(track.duration_ms, Some(recording.duration_ms as u64));
    assert!(track.artwork.is_some());
    assert_eq!(
        next(&mut events).await,
        Event::Playback(MediaPlayback::Paused { position_ms: 0 })
    );
    quiet(&mut events).await;

    sender.try_send(MediaCommand::Next).unwrap();
    assert_eq!(next(&mut events).await, Event::Clear);
    assert!(matches!(next(&mut events).await, Event::Track(_)));
    assert!(matches!(next(&mut events).await, Event::Playback(_)));

    let seen = player.inspect(&f.actor).await.unwrap().state.unwrap();
    player
        .submit(
            &f.actor,
            command(lease, 2, seen.revision, PlayerAction::Pause {}),
        )
        .await
        .expect_err("pause needs native output");
    player
        .submit(
            &f.actor,
            command(lease, 2, seen.revision, PlayerAction::Next {}),
        )
        .await
        .expect("system commands never consume the client's sequence");

    sender.try_send(MediaCommand::Stop).unwrap();
    loop {
        if next(&mut events).await == Event::Clear {
            break;
        }
    }
    quiet(&mut events).await;
    assert!(
        player
            .inspect(&f.actor)
            .await
            .unwrap()
            .state
            .unwrap()
            .current
            .is_none()
    );
    owner.close().await.unwrap();
}

#[tokio::test]
async fn oversized_artwork_is_not_published() {
    let f = fixture::LocalPlayback::with_audio(|path| {
        std::fs::write(path.parent().unwrap().join("cover.png"), png(4000, 4000)).unwrap();
        hito_core::test_support::support::write_wav(path, 0)
    })
    .await;
    let (player, owner, mut events, _sender) = start(&f);
    let lease = player
        .claim(&f.actor, player.inspect(&f.actor).await.unwrap().epoch)
        .await
        .unwrap()
        .lease;
    player
        .submit(
            &f.actor,
            command(
                lease,
                1,
                0,
                PlayerAction::Replace {
                    recordings: vec![Uuid::parse_str(&f.recording).unwrap()],
                    start: 0,
                },
            ),
        )
        .await
        .unwrap();
    let Event::Track(track) = next(&mut events).await else {
        panic!("metadata");
    };
    assert!(track.artwork.is_none());
    owner.close().await.unwrap();
}

#[tokio::test]
async fn logout_and_account_switch_clear_without_another_command() {
    let f = fixture::LocalPlayback::start().await;
    let (player, owner, mut events, _sender) = start(&f);
    let seen = player.inspect(&f.actor).await.unwrap();
    let lease = player.claim(&f.actor, seen.epoch).await.unwrap().lease;
    player
        .submit(
            &f.actor,
            command(
                lease,
                1,
                0,
                PlayerAction::Replace {
                    recordings: vec![Uuid::parse_str(&f.recording).unwrap()],
                    start: 0,
                },
            ),
        )
        .await
        .unwrap();
    assert!(matches!(next(&mut events).await, Event::Track(_)));
    assert!(matches!(next(&mut events).await, Event::Playback(_)));

    let other = f
        .services
        .identity
        .login(hito_core::Credentials {
            username: "admin".into(),
            password: "indexing test password".into(),
        })
        .await
        .unwrap();
    let other_actor = f
        .services
        .identity
        .authenticate(&other.token)
        .await
        .unwrap();
    let epoch = player.inspect(&f.actor).await.unwrap().epoch;
    player.claim(&other_actor, epoch).await.unwrap();
    assert_eq!(next(&mut events).await, Event::Clear);
    quiet(&mut events).await;

    owner.close().await.unwrap();
}

#[tokio::test]
async fn logout_clears_private_metadata_without_another_command() {
    let f = fixture::LocalPlayback::start().await;
    let (player, owner, mut events, sender) = start(&f);
    let lease = player
        .claim(&f.actor, player.inspect(&f.actor).await.unwrap().epoch)
        .await
        .unwrap()
        .lease;
    player
        .submit(
            &f.actor,
            command(
                lease,
                1,
                0,
                PlayerAction::Replace {
                    recordings: vec![Uuid::parse_str(&f.recording).unwrap()],
                    start: 0,
                },
            ),
        )
        .await
        .unwrap();
    assert!(matches!(next(&mut events).await, Event::Track(_)));
    assert!(matches!(next(&mut events).await, Event::Playback(_)));
    f.services.identity.logout(&f.token).await.unwrap();
    assert_eq!(next(&mut events).await, Event::Clear);
    sender.try_send(MediaCommand::Next).unwrap();
    quiet(&mut events).await;
    owner.close().await.unwrap();
}
