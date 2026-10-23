#![cfg(feature = "device")]
#[path = "fixtures/audiobook_queue.rs"]
mod audiobook;
use hito_audio::test_support::capture;
#[path = "fixtures/audiobook_checkpoint_events.rs"]
mod checkpoint_events;
use hito_core::test_support::playback as fixture;
use hito_core::test_support::support;

use hito_core::{Action, Actor, AudiobookChapterAnchor, Error, SyncBatch, SyncEvent};
use hito_playback::{Player, PlayerAction, PlayerCommand, PlayerPhase, PlayerQueueInput};
use serde_json::json;
use std::{collections::HashSet, time::Duration};
use tracing_subscriber::fmt::format::FmtSpan;
use uuid::Uuid;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires the isolated clocked audio monitor"]
async fn retried_native_checkpoint_keeps_both_operation_ids_and_chapter_before_later_audio() {
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
    let expected = anchors
        .iter()
        .map(|anchor| serde_json::from_value::<AudiobookChapterAnchor>(anchor.clone()).unwrap())
        .collect::<Vec<_>>();
    let before_book = serde_json::to_value(
        f.services
            .collection
            .audiobook(&actor, &book.to_string())
            .await
            .unwrap(),
    )
    .unwrap();
    let before_events = f.services.collection.events(&actor, 0).await.unwrap();
    assert!(!before_events.has_more);
    let before_admin =
        serde_json::to_value(f.services.collection.events(&f.actor, 0).await.unwrap()).unwrap();
    let pool = sqlx::SqlitePool::connect_with(
        sqlx::sqlite::SqliteConnectOptions::new().filename(f.directory.path().join("hito.db")),
    )
    .await
    .unwrap();
    sqlx::query("CREATE TRIGGER reject_audiobook_checkpoint BEFORE INSERT ON audiobook_continuations BEGIN SELECT RAISE(ABORT, 'fixture denied chapter checkpoint'); END")
        .execute(&pool).await.unwrap();
    let events = checkpoint_events::Events::default();
    let writer = events.clone();
    let dispatch = tracing::Dispatch::new(
        tracing_subscriber::fmt()
            .json()
            .with_ansi(false)
            .with_max_level(tracing::Level::DEBUG)
            .with_span_events(FmtSpan::NEW)
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
    let revision = command(
        &player,
        &actor,
        lease,
        1,
        0,
        PlayerAction::ReplaceEntries {
            entries: expected
                .iter()
                .map(|chapter| PlayerQueueInput {
                    recording_id: chapter.recording_id,
                    chapter: Some(*chapter),
                })
                .collect(),
            start: 2,
        },
    )
    .await;
    let queue = player.queue(&actor, lease, revision, 0, 3).await.unwrap();
    assert_eq!(queue.total, 3);
    assert!(
        queue
            .entries
            .iter()
            .all(|entry| !chapters.contains(&entry.id))
    );
    let mut state = player.subscribe(&actor, lease).await.unwrap();
    let mut position = player.subscribe_position(&actor, lease).await.unwrap();
    command(&player, &actor, lease, 2, revision, PlayerAction::Resume {}).await;
    tokio::time::timeout(
        Duration::from_secs(3),
        position.wait_for(|position| {
            position.entry == Some(queue.entries[2].id) && position.position_ms >= 150
        }),
    )
    .await
    .unwrap()
    .unwrap();
    capture.after(capture.mark(), 0.05);
    assert!(capture::energy(&capture.recent(0.05)) > 0.00001);
    command(&player, &actor, lease, 3, revision, PlayerAction::Pause {}).await;
    tokio::time::timeout(
        Duration::from_secs(3),
        state.wait_for(|state| {
            state.phase == PlayerPhase::Ready && !state.playing && !state.desired_playing
        }),
    )
    .await
    .unwrap()
    .unwrap();
    let first = tokio::time::timeout(Duration::from_secs(3), events.failed(1))
        .await
        .unwrap();
    assert_eq!(first[0].operations.len(), 2);
    assert_eq!(
        first[0].operations[0],
        ("progress_set".into(), first[0].progress_id)
    );
    assert_eq!(first[0].operations[1].0, "audiobook_chapter_set");
    assert_ne!(first[0].operations[0].1, first[0].operations[1].1);
    let revision = command(
        &player,
        &actor,
        lease,
        4,
        revision,
        PlayerAction::Select {
            entry: queue.entries[1].id,
        },
    )
    .await;
    command(&player, &actor, lease, 5, revision, PlayerAction::Resume {}).await;
    tokio::time::timeout(
        Duration::from_secs(3),
        position.wait_for(|position| {
            position.entry == Some(queue.entries[1].id) && position.position_ms >= 150
        }),
    )
    .await
    .unwrap()
    .unwrap();
    capture.after(capture.mark(), 0.05);
    assert!(capture::energy(&capture.recent(0.05)) > 0.00001);
    command(&player, &actor, lease, 6, revision, PlayerAction::Pause {}).await;
    tokio::time::timeout(
        Duration::from_secs(3),
        state.wait_for(|state| {
            state.phase == PlayerPhase::Ready && !state.playing && !state.desired_playing
        }),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(state.borrow().current.unwrap().chapter, Some(expected[1]));
    let failed = tokio::time::timeout(Duration::from_secs(3), events.failed(2))
        .await
        .unwrap();
    for attempt in &failed {
        assert_eq!(attempt.progress_id, first[0].progress_id);
        assert_eq!(
            attempt.operations, first[0].operations,
            "The failed A checkpoint must retain both IDs while the selected chapter changes to B"
        );
    }
    assert_eq!(
        serde_json::to_value(
            f.services
                .collection
                .audiobook(&actor, &book.to_string())
                .await
                .unwrap()
        )
        .unwrap(),
        before_book
    );
    assert_eq!(
        serde_json::to_value(f.services.collection.events(&actor, 0).await.unwrap()).unwrap(),
        serde_json::to_value(&before_events).unwrap()
    );
    assert!(
        f.services
            .collection
            .saved_playback(&actor, &recordings)
            .await
            .unwrap()
            .is_empty()
    );
    sqlx::query("DROP TRIGGER reject_audiobook_checkpoint")
        .execute(&pool)
        .await
        .unwrap();
    owner.close().await.unwrap();
    let journal = f
        .services
        .collection
        .events(&actor, before_events.cursor)
        .await
        .unwrap();
    assert!(!journal.has_more);
    assert!(journal.operations.len() >= 8);
    let (pairs, remainder) = journal.operations.as_chunks::<2>();
    assert!(remainder.is_empty());
    let mut seen_b = false;
    let mut ids = HashSet::new();
    let mut client = None;
    let mut sequence = before_events.cursor;
    let mut saved_positions = [0; 2];
    for pair in pairs {
        let [
            SyncEvent::Operation {
                sequence: progress_sequence,
                device_id,
                operation: progress,
            },
            SyncEvent::Operation {
                sequence: chapter_sequence,
                device_id: chapter_client,
                operation: chapter,
            },
        ] = pair
        else {
            panic!("Native chapter checkpoints must remain private operations");
        };
        assert!(*progress_sequence > sequence && *chapter_sequence > *progress_sequence);
        sequence = *chapter_sequence;
        assert_eq!(device_id, chapter_client);
        assert_eq!(*client.get_or_insert(device_id.as_str()), device_id);
        assert!(device_id.starts_with("native-"));
        assert!(ids.insert(progress.id) && ids.insert(chapter.id));
        let Action::ProgressSet {
            recording_id,
            position_ms,
            speed,
        } = progress.action
        else {
            panic!("The observed recording checkpoint must precede its chapter anchor");
        };
        assert!((0..=1_000).contains(&position_ms));
        assert_eq!(speed, 1.0);
        let Action::AudiobookChapterSet { chapter } = chapter.action else {
            panic!("Every observed recording checkpoint must retain its exact chapter");
        };
        assert_eq!(chapter.recording_id, recording_id);
        if chapter == expected[1] {
            seen_b = true;
            saved_positions[1] = position_ms;
        } else {
            assert_eq!(chapter, expected[2]);
            assert!(
                !seen_b,
                "A retained checkpoint must commit before later B observations"
            );
            saved_positions[0] = position_ms;
        }
    }
    assert!(seen_b);
    let [
        SyncEvent::Operation {
            device_id,
            operation: first_progress,
            ..
        },
        SyncEvent::Operation {
            operation: first_chapter,
            ..
        },
    ] = &pairs[0]
    else {
        unreachable!()
    };
    assert_eq!(first_progress.id, first[0].operations[0].1);
    assert_eq!(first_chapter.id, first[0].operations[1].1);
    let before_replay = json!({
        "book": f.services.collection.audiobook(&actor, &book.to_string()).await.unwrap(),
        "progress": f.services.collection.saved_playback(&actor, &recordings).await.unwrap(),
        "events": journal,
    });
    assert_eq!(before_replay["book"]["continuation"]["chapter"], anchors[1]);
    assert_eq!(before_replay["progress"].as_array().unwrap().len(), 2);
    for (recording, position) in recordings.into_iter().zip(saved_positions) {
        assert!(position >= 150);
        assert!(
            before_replay["progress"]
                .as_array()
                .unwrap()
                .contains(&json!({
                    "recording_id": recording, "position_ms": position, "speed": 1.0,
                }))
        );
    }
    let batch: SyncBatch = serde_json::from_value(json!({
        "device_id": device_id, "operations": [first_progress, first_chapter],
    }))
    .unwrap();
    assert_eq!(
        f.services
            .collection
            .apply(&actor, batch)
            .await
            .unwrap()
            .acknowledged,
        vec![first_progress.id, first_chapter.id]
    );
    assert_eq!(
        json!({
            "book": f.services.collection.audiobook(&actor, &book.to_string()).await.unwrap(),
            "progress": f.services.collection.saved_playback(&actor, &recordings).await.unwrap(),
            "events": f.services.collection.events(&actor, before_events.cursor).await.unwrap(),
        }),
        before_replay,
        "Replaying an earlier accepted pair must not duplicate events or move continuation back from B"
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
    pool.close().await;
}

async fn command(
    player: &Player,
    actor: &Actor,
    lease: Uuid,
    sequence: u64,
    revision: u64,
    action: PlayerAction,
) -> u64 {
    player
        .submit(
            actor,
            PlayerCommand {
                id: Uuid::new_v4(),
                lease,
                sequence,
                revision,
                action,
            },
        )
        .await
        .unwrap()
        .revision
}
