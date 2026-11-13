use hito_core::test_support::playback as fixture;
use hito_playback::{
    MediaIntegration, MediaPlayback, MediaSink, MediaTrack, Player, PlayerAction, PlayerCommand,
    media_commands,
};
use std::time::Duration;
use tokio::sync::mpsc;
use uuid::Uuid;

struct Tracks(mpsc::UnboundedSender<MediaTrack>);
impl MediaSink for Tracks {
    fn set_track(&mut self, track: &MediaTrack) {
        let _ = self.0.send(track.clone());
    }
    fn set_playback(&mut self, _: MediaPlayback) {}
    fn clear(&mut self) {}
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

async fn track(received: &mut mpsc::UnboundedReceiver<MediaTrack>) -> MediaTrack {
    tokio::time::timeout(Duration::from_secs(5), received.recv())
        .await
        .unwrap()
        .unwrap()
}

#[tokio::test]
async fn media_identity_follows_selection_and_lease_without_reusing_recording_identity() {
    let f = fixture::LocalPlayback::start().await;
    let (sent, mut received) = mpsc::unbounded_channel();
    let (_, commands) = media_commands();
    let (player, owner) = Player::start_with_media(
        f.services.clone(),
        MediaIntegration::new(Tracks(sent), commands),
    );
    let lease = player
        .claim(&f.actor, player.inspect(&f.actor).await.unwrap().epoch)
        .await
        .unwrap()
        .lease;
    let recording = Uuid::parse_str(&f.recording).unwrap();
    let first = player
        .submit(
            &f.actor,
            command(
                lease,
                1,
                0,
                PlayerAction::Replace {
                    recordings: vec![recording; 2],
                    start: 0,
                },
            ),
        )
        .await
        .unwrap();
    let a = track(&mut received).await;
    assert!(!a.id.is_nil());
    let next = player
        .submit(
            &f.actor,
            command(lease, 2, first.revision, PlayerAction::Next {}),
        )
        .await
        .unwrap();
    let b = track(&mut received).await;
    assert_ne!(a.id, b.id);
    assert_eq!(a.title, b.title);
    let current = player
        .inspect(&f.actor)
        .await
        .unwrap()
        .state
        .unwrap()
        .current
        .unwrap();
    player
        .submit(
            &f.actor,
            command(
                lease,
                3,
                next.revision,
                PlayerAction::Select { entry: current.id },
            ),
        )
        .await
        .unwrap();
    let replay = track(&mut received).await;
    assert_ne!(b.id, replay.id);
    let replacement = player
        .claim(&f.actor, player.inspect(&f.actor).await.unwrap().epoch)
        .await
        .unwrap();
    assert_ne!(replacement.lease, lease);
    let reclaimed = track(&mut received).await;
    assert_ne!(replay.id, reclaimed.id);
    assert_eq!(
        std::fs::read(f.music.join("track.wav")).unwrap(),
        f.original
    );
    owner.close().await.unwrap();
}

#[cfg(feature = "device")]
mod native {
    use super::*;
    use futures_util::FutureExt;
    use hito_playback::{MediaCommand, PlayerPhase};
    use std::sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc as blocking,
    };
    use tracing::{
        Event, Subscriber,
        field::{Field, Visit},
    };
    use tracing_subscriber::{Layer, layer::Context, prelude::*};

    #[derive(Default)]
    struct Fields {
        event: String,
        scoped: bool,
        success: bool,
    }
    impl Visit for Fields {
        fn record_str(&mut self, field: &Field, value: &str) {
            if field.name() == "event" {
                self.event = value.to_owned();
            }
        }
        fn record_bool(&mut self, field: &Field, value: bool) {
            match field.name() {
                "scoped_seek" => self.scoped = value,
                "success" => self.success = value,
                _ => (),
            }
        }
        fn record_debug(&mut self, _: &Field, _: &dyn std::fmt::Debug) {}
    }

    struct Observe {
        armed: Arc<AtomicBool>,
        entered: mpsc::UnboundedSender<()>,
        release: Mutex<blocking::Receiver<()>>,
        finished: mpsc::UnboundedSender<bool>,
    }
    impl<S: Subscriber> Layer<S> for Observe {
        fn on_event(&self, event: &Event<'_>, _: Context<'_, S>) {
            let mut fields = Fields::default();
            event.record(&mut fields);
            if fields.event == "media_command_dispatching"
                && fields.scoped
                && self.armed.swap(false, Ordering::SeqCst)
            {
                self.entered.send(()).unwrap();
                self.release
                    .lock()
                    .unwrap()
                    .recv_timeout(Duration::from_secs(5))
                    .expect("release owned dispatch observation");
            }
            if fields.event == "media_command_finished" && fields.scoped {
                let _ = self.finished.send(fields.success);
            }
        }
    }

    struct Release(blocking::Sender<()>);
    impl Drop for Release {
        fn drop(&mut self) {
            let _ = self.0.send(());
        }
    }

    struct DelayedTracks {
        sent: mpsc::UnboundedSender<MediaTrack>,
        held: Arc<AtomicBool>,
        entered: mpsc::UnboundedSender<()>,
        release: blocking::Receiver<()>,
        delivered: Option<Uuid>,
    }
    impl MediaSink for DelayedTracks {
        fn set_track(&mut self, track: &MediaTrack) {
            if self.delivered != Some(track.id) && self.held.swap(false, Ordering::SeqCst) {
                self.entered.send(()).unwrap();
                self.release
                    .recv_timeout(Duration::from_secs(5))
                    .expect("release owned metadata publication");
            }
            self.delivered = Some(track.id);
            let _ = self.sent.send(track.clone());
        }
        fn set_playback(&mut self, _: MediaPlayback) {}
        fn clear(&mut self) {}
    }

    async fn completion(received: &mut mpsc::UnboundedReceiver<bool>) -> bool {
        tokio::time::timeout(Duration::from_secs(5), received.recv())
            .await
            .unwrap()
            .unwrap()
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    #[ignore = "requires the isolated clocked audio monitor"]
    async fn queued_scoped_seek_keeps_its_original_track_when_metadata_delivery_and_dispatch_are_delayed()
     {
        let f = fixture::LocalPlayback::start().await;
        hito_testkit::logs::anchor_callsite_interest();
        let armed = Arc::new(AtomicBool::new(false));
        let (entered, mut at_dispatch) = mpsc::unbounded_channel();
        let (dispatch_release, dispatch_gate) = blocking::channel();
        let dispatch_release = Release(dispatch_release);
        let (finished, mut completed) = mpsc::unbounded_channel();
        let observer = Observe {
            armed: armed.clone(),
            entered,
            release: Mutex::new(dispatch_gate),
            finished,
        };
        let dispatch = tracing::Dispatch::new(tracing_subscriber::registry().with(observer));
        let held = Arc::new(AtomicBool::new(false));
        let (sink_entered, mut at_sink) = mpsc::unbounded_channel();
        let (sink_release, sink_gate) = blocking::channel();
        let sink_release = Release(sink_release);
        let (sent, mut received) = mpsc::unbounded_channel();
        let (sender, commands) = media_commands();
        let (player, owner) = tracing::dispatcher::with_default(&dispatch, || {
            Player::native_with_media(
                f.services.clone(),
                MediaIntegration::new(
                    DelayedTracks {
                        sent,
                        held: held.clone(),
                        entered: sink_entered,
                        release: sink_gate,
                        delivered: None,
                    },
                    commands,
                ),
            )
        });
        let lease = player
            .claim(&f.actor, player.inspect(&f.actor).await.unwrap().epoch)
            .await
            .unwrap()
            .lease;
        let recording = Uuid::parse_str(&f.recording).unwrap();
        player
            .submit(
                &f.actor,
                command(
                    lease,
                    1,
                    0,
                    PlayerAction::Replace {
                        recordings: vec![recording; 2],
                        start: 0,
                    },
                ),
            )
            .await
            .unwrap();
        let mut state = player.subscribe(&f.actor, lease).await.unwrap();
        tokio::time::timeout(
            Duration::from_secs(5),
            state.wait_for(|state| state.phase == PlayerPhase::Ready),
        )
        .await
        .unwrap()
        .unwrap();
        let a = track(&mut received).await;
        sender
            .try_send(MediaCommand::SeekTrack {
                track: a.id,
                position_us: 100_000,
            })
            .unwrap();
        assert!(
            completion(&mut completed).await,
            "current-track scoped seek must be admitted"
        );
        let mut position = player.subscribe_position(&f.actor, lease).await.unwrap();
        tokio::time::timeout(
            Duration::from_secs(5),
            position.wait_for(|position| position.position_ms == 100),
        )
        .await
        .unwrap()
        .unwrap();
        let mut current = a;
        for (sequence, restart) in [(2, false), (3, true)] {
            let before = player.inspect(&f.actor).await.unwrap().state.unwrap();
            held.store(true, Ordering::SeqCst);
            armed.store(true, Ordering::SeqCst);
            sender
                .try_send(MediaCommand::SeekTrack {
                    track: current.id,
                    position_us: 400_000,
                })
                .unwrap();
            tokio::time::timeout(Duration::from_secs(5), at_dispatch.recv())
                .await
                .unwrap()
                .unwrap();
            let action = if restart {
                PlayerAction::Select {
                    entry: before.current.unwrap().id,
                }
            } else {
                PlayerAction::Next {}
            };
            let next = player.submit(&f.actor, command(lease, sequence, before.revision, action));
            tokio::pin!(next);
            assert!(
                next.as_mut().now_or_never().is_none(),
                "client selection is queued before the captured host request"
            );
            dispatch_release.0.send(()).unwrap();
            tokio::time::timeout(Duration::from_secs(5), &mut next)
                .await
                .unwrap()
                .unwrap();
            tokio::time::timeout(Duration::from_secs(5), at_sink.recv())
                .await
                .unwrap()
                .unwrap();
            assert!(
                !completion(&mut completed).await,
                "the queued seek must retain its original occurrence"
            );
            while let Ok(delivered) = received.try_recv() {
                assert_eq!(
                    delivered.id, current.id,
                    "new occurrence metadata remains held outside the player owner"
                );
            }
            sink_release.0.send(()).unwrap();
            let next_track = track(&mut received).await;
            assert_ne!(current.id, next_track.id);
            tokio::time::timeout(
                Duration::from_secs(5),
                state.wait_for(|state| {
                    state.phase == PlayerPhase::Ready && state.current_index == Some(1)
                }),
            )
            .await
            .unwrap()
            .unwrap();
            let selected = player.inspect(&f.actor).await.unwrap().state.unwrap();
            let entry = selected.current.unwrap().id;
            let selection = selected.selection;
            tokio::time::timeout(
                Duration::from_secs(5),
                position.wait_for(|position| {
                    position.entry == Some(entry)
                        && position.selection == selection
                        && position.position_ms == 0
                }),
            )
            .await
            .unwrap()
            .unwrap();
            sender
                .try_send(MediaCommand::SeekTrack {
                    track: next_track.id,
                    position_us: 300_000,
                })
                .unwrap();
            assert!(
                completion(&mut completed).await,
                "the new current identity remains usable"
            );
            tokio::time::timeout(
                Duration::from_secs(5),
                position.wait_for(|position| position.position_ms == 300),
            )
            .await
            .unwrap()
            .unwrap();
            current = next_track;
        }
        sender
            .try_send(MediaCommand::SeekTrack {
                track: current.id,
                position_us: 1_000_001,
            })
            .unwrap();
        assert!(
            !completion(&mut completed).await,
            "submillisecond overflow is rejected before conversion"
        );
        assert_eq!(position.borrow().position_ms, 300);
        sender
            .try_send(MediaCommand::SeekTrack {
                track: current.id,
                position_us: 200_000,
            })
            .unwrap();
        assert!(completion(&mut completed).await);
        tokio::time::timeout(
            Duration::from_secs(5),
            position.wait_for(|position| position.position_ms == 200),
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(
            std::fs::read(f.music.join("track.wav")).unwrap(),
            f.original
        );
        owner.close().await.unwrap();
    }
}
