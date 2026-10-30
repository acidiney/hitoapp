#![cfg(feature = "device")]
#[path = "../fixtures/audiobook_queue.rs"]
mod audiobook;
use hito_audio::test_support::capture;
use hito_core::test_support::playback as fixture;
use hito_core::test_support::support;

use hito_core::{Action, Actor, AudiobookChapterAnchor, Error, SyncEvent};
use hito_playback::{Player, PlayerAction, PlayerCommand, PlayerPhase, PlayerQueueInput};
use std::{collections::HashSet, time::Duration};
use uuid::Uuid;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires the isolated clocked audio monitor"]
async fn a_paused_seek_saves_recording_progress_but_selects_the_chapter_only_after_hearing_it() {
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
    let capture = capture::Capture::open();
    let (player, owner) = Player::native(f.services.clone());
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
    submit(&player, &actor, lease, 2, PlayerAction::Speed { rate: 500 }).await;
    let seek = submit(
        &player,
        &actor,
        lease,
        3,
        PlayerAction::Seek { position_ms: 100 },
    )
    .await;
    tokio::time::timeout(
        Duration::from_secs(3),
        position.wait_for(|position| {
            position.entry == Some(selected.id)
                && position.seek_id == Some(seek)
                && position.position_ms == 100
        }),
    )
    .await
    .unwrap()
    .unwrap();
    let mut last_saved = Vec::new();
    let saved = tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            last_saved = f
                .services
                .collection
                .saved_playback(&actor, &recordings)
                .await
                .unwrap();
            if last_saved.len() == 1
                && last_saved[0].position_ms == 100
                && last_saved[0].speed == 1.0
            {
                assert_eq!(last_saved[0].recording_id, recordings[0]);
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await;
    assert!(
        saved.is_ok(),
        "Paused seek checkpoint did not match: saved={last_saved:?}, state={:?}, position_ms={}, seek_id={:?}",
        *state.borrow(),
        position.borrow().position_ms,
        position.borrow().seek_id
    );
    capture.after(capture.mark(), 0.1);
    assert!(capture::energy(&capture.recent(0.1)) < 0.000001);
    let paused = state.borrow().clone();
    assert_eq!(paused.current, Some(selected));
    assert!(!paused.playing && !paused.desired_playing && !paused.seeking);
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
        "An explicit seek must not select a chapter that has never produced audio"
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
        panic!("A paused recording seek must remain private");
    };
    assert!(matches!(operation.action, Action::ProgressSet {
        recording_id, position_ms: 100, speed: 1.0
    } if recording_id == recordings[0]));
    submit(&player, &actor, lease, 4, PlayerAction::Resume {}).await;
    tokio::time::timeout(
        Duration::from_secs(3),
        position.wait_for(|position| {
            position.entry == Some(selected.id) && position.position_ms >= 150
        }),
    )
    .await
    .unwrap()
    .unwrap();
    capture.after(capture.mark(), 0.05);
    assert!(capture::energy(&capture.recent(0.05)) > 0.00001);
    let heard = tokio::time::timeout(Duration::from_secs(1), async {
        loop {
            let detail = f
                .services
                .collection
                .audiobook(&actor, &book.to_string())
                .await
                .unwrap();
            if detail.continuation.is_some() {
                break detail;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("The first heard chapter must not wait for the five-second checkpoint timer");
    let playing = player.inspect(&actor).await.unwrap().state.unwrap();
    assert_eq!(playing.current, Some(selected));
    assert!(playing.playing && playing.desired_playing);
    let continuation = heard.continuation.unwrap();
    assert_eq!(continuation.chapter, expected);
    assert_eq!(continuation.progress.unwrap().speed, 0.5);
    let heard_events = f
        .services
        .collection
        .events(&actor, seek_events.cursor)
        .await
        .unwrap();
    assert!(!heard_events.has_more);
    let (pairs, remainder) = heard_events.operations.as_chunks::<2>();
    assert!(remainder.is_empty() && (1..=2).contains(&pairs.len()));
    let mut ids = HashSet::new();
    let mut client = None;
    let mut sequence = seek_events.cursor;
    let mut position_ms = 100;
    let mut speed = 1.0;
    for (index, pair) in pairs.iter().enumerate() {
        let [
            SyncEvent::Operation {
                sequence: progress_sequence,
                device_id: progress_client,
                operation: progress,
            },
            SyncEvent::Operation {
                sequence: chapter_sequence,
                device_id: chapter_client,
                operation: chapter,
            },
        ] = pair
        else {
            panic!(
                "The first heard observation and observed rate change must remain private paired checkpoints"
            );
        };
        assert!(*progress_sequence > sequence && *chapter_sequence > *progress_sequence);
        sequence = *chapter_sequence;
        assert_eq!(progress_client, chapter_client);
        assert_eq!(
            *client.get_or_insert(progress_client.as_str()),
            progress_client
        );
        assert!(ids.insert(progress.id) && ids.insert(chapter.id));
        let Action::ProgressSet {
            recording_id,
            position_ms: observed_position,
            speed: observed_speed,
        } = progress.action
        else {
            panic!("The recording checkpoint must precede its exact chapter anchor");
        };
        assert_eq!(recording_id, recordings[0]);
        assert!((position_ms..1_000).contains(&observed_position));
        assert_eq!(
            observed_speed,
            if index + 1 == pairs.len() { 0.5 } else { 1.0 }
        );
        position_ms = observed_position;
        speed = observed_speed;
        assert!(
            matches!(chapter.action, Action::AudiobookChapterSet { chapter } if chapter == expected)
        );
    }
    assert_eq!(speed, 0.5);
    owner.close().await.unwrap();
    for recording in recordings {
        assert!(matches!(
            f.services
                .downloads
                .status(&actor, &recording.to_string())
                .await,
            Err(Error::NotFound)
        ));
    }
    assert!(
        f.services
            .collection
            .activity(&f.actor)
            .await
            .unwrap()
            .history
            .is_empty()
    );
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
