#![cfg(feature = "device")]
#[path = "../fixtures/audiobook_queue.rs"]
mod audiobook;
use hito_audio::test_support::capture;
#[path = "../fixtures/event_gate.rs"]
mod event_gate;
use hito_core::test_support::playback as fixture;
use hito_core::test_support::support;

use hito_core::{Action, Actor, AudiobookChapterAnchor, Error, SyncEvent};
use hito_playback::{Player, PlayerAction, PlayerCommand, PlayerPhase, PlayerQueueInput};
use std::time::Duration;
use uuid::Uuid;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires the isolated clocked audio monitor"]
async fn a_silent_seek_and_coalesced_resume_at_eof_never_select_an_unheard_audiobook_chapter() {
    let audiobook::Library {
        source: f,
        actor,
        recordings,
        book,
        chapters,
        anchors,
        second_path,
        second_original,
    } = audiobook::Library::start().await;
    let expected: AudiobookChapterAnchor = serde_json::from_value(anchors[2].clone()).unwrap();
    let before_book = serde_json::to_value(
        f.services
            .collection
            .audiobook(&actor, &book.to_string())
            .await
            .unwrap(),
    )
    .unwrap();
    let before = f.services.collection.events(&actor, 0).await.unwrap();
    assert!(!before.has_more);
    let before_admin =
        serde_json::to_value(f.services.collection.events(&f.actor, 0).await.unwrap()).unwrap();
    let controls = event_gate::EventGate::new("player_controls_applied");
    let _release = controls.guard();
    let writer = controls.clone();
    hito_testkit::logs::anchor_callsite_interest();
    let dispatch = tracing::Dispatch::new(
        tracing_subscriber::fmt()
            .json()
            .with_ansi(false)
            .with_max_level(tracing::Level::DEBUG)
            .with_writer(move || writer.clone())
            .finish(),
    );
    let capture = capture::Capture::open();
    let (player, owner) =
        tracing::dispatcher::with_default(&dispatch, || Player::native(f.services.clone()));
    let lease = player
        .claim(&actor, player.inspect(&actor).await.unwrap().epoch)
        .await
        .unwrap()
        .lease;
    submit(
        &player,
        &actor,
        lease,
        1,
        PlayerAction::ReplaceEntries {
            entries: anchors
                .iter()
                .map(|anchor| {
                    let chapter: AudiobookChapterAnchor =
                        serde_json::from_value(anchor.clone()).unwrap();
                    PlayerQueueInput {
                        recording_id: chapter.recording_id,
                        chapter: Some(chapter),
                    }
                })
                .collect(),
            start: 2,
        },
    )
    .await;
    let mut state = player.subscribe(&actor, lease).await.unwrap();
    let mut position = player.subscribe_position(&actor, lease).await.unwrap();
    tokio::time::timeout(
        Duration::from_secs(3),
        state.wait_for(|state| {
            state.phase == PlayerPhase::Ready && !state.playing && !state.desired_playing
        }),
    )
    .await
    .unwrap()
    .unwrap();
    let selected = state.borrow().current.unwrap();
    assert_eq!(selected.chapter, Some(expected));
    assert!(!chapters.contains(&selected.id));
    let seek = submit(
        &player,
        &actor,
        lease,
        2,
        PlayerAction::Seek { position_ms: 1_000 },
    )
    .await;
    tokio::time::timeout(Duration::from_secs(3), position.wait_for(|position| {
        position.entry == Some(selected.id) && position.seek_id == Some(seek)
            && position.position_ms == 1_000 && position.duration_ms == Some(1_000)
    })).await.unwrap_or_else(|_| panic!(
        "The owned fixture must complete its paused EOF seek: state={:?}, expected_seek_id={seek}",
        *state.borrow()
    )).unwrap();
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            let saved = f
                .services
                .collection
                .saved_playback(&actor, &recordings)
                .await
                .unwrap();
            if saved.len() == 1 && saved[0].position_ms == 1_000 {
                assert_eq!(saved[0].recording_id, recordings[0]);
                assert_eq!(saved[0].speed, 1.0);
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    capture.after(capture.mark(), 0.1);
    assert!(capture.all().iter().all(|sample| sample.abs() < 0.000001));
    assert!(!state.borrow().playing && !state.borrow().desired_playing);
    assert_eq!(
        serde_json::to_value(
            f.services
                .collection
                .audiobook(&actor, &book.to_string())
                .await
                .unwrap()
        )
        .unwrap(),
        before_book,
        "A silent seek to EOF must not select an unheard chapter"
    );
    let seek_events = f
        .services
        .collection
        .events(&actor, before.cursor)
        .await
        .unwrap();
    assert!(!seek_events.has_more);
    assert_eq!(seek_events.operations.len(), 1);
    let SyncEvent::Operation { operation, .. } = &seek_events.operations[0] else {
        panic!("An explicit paused EOF seek must remain one private recording checkpoint");
    };
    assert!(matches!(operation.action, Action::ProgressSet {
        recording_id, position_ms: 1_000, speed: 1.0
    } if recording_id == recordings[0]));
    controls.arm();
    submit(&player, &actor, lease, 3, PlayerAction::Resume {}).await;
    tokio::time::timeout(Duration::from_secs(3), controls.entered())
        .await
        .unwrap();
    capture.after(capture.mark(), 0.3);
    assert!(
        capture.all().iter().all(|sample| sample.abs() < 0.000001),
        "Resuming exactly at EOF must not produce audio frames"
    );
    controls.release();
    tokio::time::timeout(
        Duration::from_secs(3),
        state.wait_for(|state| state.current.is_none()),
    )
    .await
    .unwrap()
    .unwrap();
    owner.close().await.unwrap();
    assert_eq!(
        serde_json::to_value(
            f.services
                .collection
                .audiobook(&actor, &book.to_string())
                .await
                .unwrap()
        )
        .unwrap(),
        before_book,
        "An EOF end marker without advancement or audio must not become a heard chapter"
    );
    assert_eq!(
        serde_json::to_value(
            f.services
                .collection
                .events(&actor, before.cursor)
                .await
                .unwrap()
        )
        .unwrap(),
        serde_json::to_value(&seek_events).unwrap(),
        "An unadvanced EOF completion must not add a chapter operation or duplicate recording progress"
    );
    assert_eq!(
        serde_json::to_value(f.services.collection.events(&f.actor, 0).await.unwrap()).unwrap(),
        before_admin
    );
    assert!(
        f.services
            .collection
            .activity(&f.actor)
            .await
            .unwrap()
            .history
            .is_empty()
    );
    for recording in recordings {
        assert!(matches!(
            f.services
                .downloads
                .status(&actor, &recording.to_string())
                .await,
            Err(Error::NotFound)
        ));
    }
    assert_eq!(
        std::fs::read(f.music.join("track.wav")).unwrap(),
        f.original
    );
    assert_eq!(std::fs::read(second_path).unwrap(), second_original);
}

async fn submit(
    player: &Player,
    actor: &Actor,
    lease: Uuid,
    sequence: u64,
    action: PlayerAction,
) -> Uuid {
    let command = PlayerCommand {
        id: Uuid::new_v4(),
        lease,
        sequence,
        revision: u64::from(sequence > 1),
        action,
    };
    let id = command.id;
    player.submit(actor, command).await.unwrap();
    id
}
