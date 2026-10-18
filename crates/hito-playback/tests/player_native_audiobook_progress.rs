#![cfg(feature = "device")]
#[path = "fixtures/audiobook_queue.rs"]
mod audiobook;
#[path = "../../hito-audio/tests/support/capture.rs"]
#[allow(dead_code)]
mod capture;
#[path = "../../hito-core/tests/fixtures/playback.rs"]
mod fixture;
#[path = "../../hito-core/tests/support/mod.rs"]
mod support;

use hito_core::{Action, AudiobookChapterAnchor, Error, SyncEvent};
use hito_playback::{Player, PlayerAction, PlayerCommand, PlayerPhase, PlayerQueueInput};
use std::{collections::HashSet, time::Duration};
use uuid::Uuid;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires the isolated clocked audio monitor"]
async fn observed_native_progress_retains_the_selected_duplicate_audiobook_chapter() {
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
    let book_id = book.to_string();
    let before_book = serde_json::to_value(
        f.services
            .collection
            .audiobook(&actor, &book_id)
            .await
            .unwrap(),
    )
    .unwrap();
    let before_events = f.services.collection.events(&actor, 0).await.unwrap();
    assert!(!before_events.has_more);
    let before_admin =
        serde_json::to_value(f.services.collection.events(&f.actor, 0).await.unwrap()).unwrap();
    let expected: AudiobookChapterAnchor = serde_json::from_value(anchors[2].clone()).unwrap();
    let entries = anchors
        .iter()
        .map(|anchor| {
            let chapter: AudiobookChapterAnchor = serde_json::from_value(anchor.clone()).unwrap();
            PlayerQueueInput {
                recording_id: chapter.recording_id,
                chapter: Some(chapter),
            }
        })
        .collect();
    let capture = capture::Capture::open();
    let (player, owner) = Player::native(f.services.clone());
    let attachment = player
        .claim(&actor, player.inspect(&actor).await.unwrap().epoch)
        .await
        .unwrap();
    let replacement = player
        .submit(
            &actor,
            PlayerCommand {
                id: Uuid::new_v4(),
                lease: attachment.lease,
                sequence: 1,
                revision: 0,
                action: PlayerAction::ReplaceEntries { entries, start: 2 },
            },
        )
        .await
        .unwrap();
    let mut state = player.subscribe(&actor, attachment.lease).await.unwrap();
    tokio::time::timeout(
        Duration::from_secs(3),
        state.wait_for(|state| state.phase == PlayerPhase::Ready && !state.playing),
    )
    .await
    .unwrap()
    .unwrap();
    let selected = state.borrow().current.unwrap();
    assert_eq!(selected.chapter, Some(expected));
    assert_eq!(selected.recording_id, recordings[0]);
    assert!(!chapters.contains(&selected.id));
    let page = player
        .queue(&actor, attachment.lease, replacement.revision, 0, 3)
        .await
        .unwrap();
    assert_eq!(page.total, 3);
    assert_eq!(page.entries[2].id, selected.id);
    assert_ne!(page.entries[0].id, selected.id);
    assert_ne!(page.entries[0].chapter, Some(expected));
    capture.after(capture.mark(), 0.1);
    assert!(capture::energy(&capture.recent(0.1)) < 0.000001);
    assert_eq!(
        serde_json::to_value(
            f.services
                .collection
                .audiobook(&actor, &book_id)
                .await
                .unwrap()
        )
        .unwrap(),
        before_book,
        "Selecting a paused chapter must not invent an observed continuation"
    );
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
        serde_json::to_value(&before_events).unwrap()
    );
    let mut position = player
        .subscribe_position(&actor, attachment.lease)
        .await
        .unwrap();
    player
        .submit(
            &actor,
            PlayerCommand {
                id: Uuid::new_v4(),
                lease: attachment.lease,
                sequence: 2,
                revision: replacement.revision,
                action: PlayerAction::Resume {},
            },
        )
        .await
        .unwrap();
    tokio::time::timeout(
        Duration::from_secs(3),
        position.wait_for(|position| position.position_ms >= 150),
    )
    .await
    .unwrap()
    .unwrap();
    capture.after(capture.mark(), 0.1);
    assert!(capture::energy(&capture.recent(0.1)) > 0.00001);
    player
        .submit(
            &actor,
            PlayerCommand {
                id: Uuid::new_v4(),
                lease: attachment.lease,
                sequence: 3,
                revision: replacement.revision,
                action: PlayerAction::Pause {},
            },
        )
        .await
        .unwrap();
    tokio::time::timeout(
        Duration::from_secs(3),
        state.wait_for(|state| {
            state.phase == PlayerPhase::Ready && !state.playing && !state.desired_playing
        }),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(state.borrow().current, Some(selected));
    owner.close().await.unwrap();
    let journal = f
        .services
        .collection
        .events(&actor, before_events.cursor)
        .await
        .unwrap();
    assert!(!journal.has_more);
    assert!(journal.operations.len() >= 4);
    let pairs = journal.operations.as_chunks::<2>();
    assert!(
        pairs.1.is_empty(),
        "Every observed checkpoint must commit its chapter anchor"
    );
    let mut client = None;
    let mut ids = HashSet::new();
    let mut sequence = before_events.cursor;
    let mut last_position = 0;
    for pair in pairs.0 {
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
            panic!("Native checkpoints must remain private operations");
        };
        assert!(*progress_sequence > sequence);
        assert!(*chapter_sequence > *progress_sequence);
        sequence = *chapter_sequence;
        assert!(progress_client.starts_with("native-"));
        assert_eq!(chapter_client, progress_client);
        assert_eq!(
            *client.get_or_insert(progress_client.as_str()),
            progress_client
        );
        assert!(ids.insert(progress.id));
        assert!(ids.insert(chapter.id));
        let Action::ProgressSet {
            recording_id,
            position_ms,
            speed,
        } = progress.action
        else {
            panic!("The recording checkpoint must precede its exact chapter anchor");
        };
        assert_eq!(recording_id, recordings[0]);
        assert!((last_position..=1_000).contains(&position_ms));
        assert_eq!(speed, 1.0);
        last_position = position_ms;
        assert!(
            matches!(chapter.action, Action::AudiobookChapterSet { chapter } if chapter == expected)
        );
    }
    assert!(last_position >= 150);
    let saved = f
        .services
        .collection
        .saved_playback(&actor, &recordings)
        .await
        .unwrap();
    assert_eq!(saved.len(), 1);
    assert_eq!(saved[0].recording_id, recordings[0]);
    assert_eq!(saved[0].position_ms, last_position as u64);
    assert_eq!(saved[0].speed, 1.0);
    let mut expected_book = before_book;
    expected_book["continuation"] = serde_json::json!({"chapter": expected, "progress": saved[0]});
    assert_eq!(
        serde_json::to_value(
            f.services
                .collection
                .audiobook(&actor, &book_id)
                .await
                .unwrap()
        )
        .unwrap(),
        expected_book
    );
    assert_eq!(
        serde_json::to_value(
            f.services
                .collection
                .events(&actor, before_events.cursor)
                .await
                .unwrap()
        )
        .unwrap(),
        serde_json::to_value(&journal).unwrap()
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
