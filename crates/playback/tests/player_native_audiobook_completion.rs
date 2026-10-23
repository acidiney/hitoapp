#![cfg(feature = "device")]
#[path = "fixtures/audiobook_queue.rs"]
mod audiobook;
use hito_audio::test_support::capture;
#[path = "fixtures/event_gate.rs"]
mod event_gate;
use hito_core::test_support::playback as fixture;
use hito_core::test_support::support;

use hito_core::{Action, AudiobookChapterAnchor, Error, SyncEvent};
use hito_playback::{Player, PlayerAction, PlayerCommand, PlayerQueueInput};
use std::time::Duration;
use tracing_subscriber::fmt::writer::MakeWriterExt;
use uuid::Uuid;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires the isolated clocked audio monitor"]
async fn a_heard_chapter_retains_its_anchor_when_the_first_observation_is_already_completed() {
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
    let opening = event_gate::EventGate::new("audio_decoder_ready");
    opening.arm();
    let _opening_release = opening.guard();
    let controls = event_gate::EventGate::new("player_controls_applied");
    let _controls_release = controls.guard();
    let opening_writer = opening.clone();
    let controls_writer = controls.clone();
    let dispatch = tracing::Dispatch::new(
        tracing_subscriber::fmt()
            .json()
            .with_ansi(false)
            .with_max_level(tracing::Level::DEBUG)
            .with_writer((move || opening_writer.clone()).and(move || controls_writer.clone()))
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
    player
        .submit(
            &actor,
            PlayerCommand {
                id: Uuid::new_v4(),
                lease,
                sequence: 1,
                revision: 0,
                action: PlayerAction::ReplaceEntries {
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
            },
        )
        .await
        .unwrap();
    let mut state = player.subscribe(&actor, lease).await.unwrap();
    let selected = state.borrow().current.unwrap();
    assert_eq!(selected.chapter, Some(expected));
    assert!(!chapters.contains(&selected.id));
    tokio::time::timeout(Duration::from_secs(3), opening.entered())
        .await
        .unwrap();
    assert!(
        f.services
            .collection
            .saved_playback(&actor, &recordings)
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        serde_json::to_value(f.services.collection.events(&actor, 0).await.unwrap()).unwrap(),
        serde_json::to_value(&before).unwrap()
    );
    controls.arm();
    player
        .submit(
            &actor,
            PlayerCommand {
                id: Uuid::new_v4(),
                lease,
                sequence: 2,
                revision: 1,
                action: PlayerAction::Resume {},
            },
        )
        .await
        .unwrap();
    opening.release();
    tokio::time::timeout(Duration::from_secs(3), controls.entered())
        .await
        .unwrap();
    capture.after(capture.mark(), 1.3);
    assert!(
        capture::energy(&capture.all()) > 0.00001,
        "The complete selected chapter must actually reach native output before its first observation"
    );
    assert!(capture::energy(&capture.recent(0.1)) < 0.000001);
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
        "Output not yet observed by the playback owner must not create a chapter checkpoint"
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
    let saved = f
        .services
        .collection
        .saved_playback(&actor, &recordings)
        .await
        .unwrap();
    assert_eq!(saved.len(), 1);
    assert_eq!(saved[0].recording_id, recordings[0]);
    assert_eq!(saved[0].position_ms, 1_000);
    assert_eq!(saved[0].speed, 1.0);
    let mut expected_book = before_book;
    expected_book["continuation"] = serde_json::json!({"chapter": expected, "progress": saved[0]});
    assert_eq!(
        serde_json::to_value(
            f.services
                .collection
                .audiobook(&actor, &book.to_string())
                .await
                .unwrap()
        )
        .unwrap(),
        expected_book,
        "A completed heard chapter must retain its exact second-A continuation even without a playing observation"
    );
    let journal = f
        .services
        .collection
        .events(&actor, before.cursor)
        .await
        .unwrap();
    assert!(!journal.has_more);
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
    ] = journal.operations.as_slice()
    else {
        panic!("One completed observed chapter must commit one private progress and anchor pair");
    };
    assert!(*progress_sequence > before.cursor && *chapter_sequence > *progress_sequence);
    assert!(progress_client.starts_with("native-"));
    assert_eq!(progress_client, chapter_client);
    assert_ne!(progress.id, chapter.id);
    assert!(
        matches!(progress.action, Action::ProgressSet { recording_id, position_ms: 1_000, speed: 1.0 }
        if recording_id == recordings[0])
    );
    assert!(
        matches!(chapter.action, Action::AudiobookChapterSet { chapter } if chapter == expected)
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
