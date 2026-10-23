#[path = "fixtures/audiobook_admission_gate.rs"]
mod admission_gate;
#[path = "fixtures/audiobook_queue.rs"]
mod audiobook;
use hito_core::test_support::playback as fixture;
use hito_core::test_support::support;

use hito_core::{Action, AudiobookChapterAnchor, Error};
use hito_playback::{Player, PlayerAction, PlayerAudiobookRequest, PlayerCommand, PlayerError};
use std::time::Duration;
use tracing::instrument::WithSubscriber;
use tracing_subscriber::layer::SubscriberExt;
use uuid::Uuid;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn deletion_and_exact_chapter_publication_share_one_authority_order() {
    let f = audiobook::Library::start().await;
    let [a, b] = f.recordings;
    let anchor: AudiobookChapterAnchor = serde_json::from_value(f.anchors[2].clone()).unwrap();
    support::synchronize(
        &f.source.services,
        &f.actor,
        vec![
            Action::ProgressSet {
                recording_id: a,
                position_ms: 500,
                speed: 1.25,
            },
            Action::AudiobookChapterSet { chapter: anchor },
        ],
    )
    .await;
    let gate = admission_gate::AdmissionGate::default();
    let _release = gate.guard();
    let dispatch = tracing::Dispatch::new(tracing_subscriber::registry().with(gate.clone()));
    let (player, owner) =
        tracing::dispatcher::with_default(&dispatch, || Player::start(f.source.services.clone()));
    let attachment = player
        .claim(&f.actor, player.inspect(&f.actor).await.unwrap().epoch)
        .await
        .unwrap();
    let old = PlayerCommand {
        id: Uuid::new_v4(),
        lease: attachment.lease,
        sequence: 1,
        revision: 0,
        action: PlayerAction::Replace {
            recordings: vec![b],
            start: 0,
        },
    };
    let old_receipt = player.submit(&f.actor, old.clone()).await.unwrap();
    let mut observed = player.subscribe(&f.actor, attachment.lease).await.unwrap();
    let before = observed.borrow().clone();
    let input = PlayerAudiobookRequest {
        id: Uuid::new_v4(),
        lease: attachment.lease,
        revision: before.revision,
        selection: before.selection,
        audiobook_id: f.book,
    };
    let prepared = player.prepare_audiobook(&f.actor, input).await.unwrap();
    assert_eq!(prepared.chapter, anchor);
    gate.arm();
    let commit = PlayerCommand {
        id: Uuid::new_v4(),
        lease: attachment.lease,
        sequence: 2,
        revision: before.revision,
        action: PlayerAction::ContinueAudiobook {
            token: prepared.token,
        },
    };
    let submitting = tokio::spawn({
        let player = player.clone();
        let actor = f.actor.clone();
        let commit = commit.clone();
        async move { player.submit(&actor, commit).await }
    });
    tokio::time::timeout(Duration::from_secs(3), gate.entered())
        .await
        .unwrap();
    let mut deleting = tokio::spawn({
        let services = f.source.services.clone();
        let actor = f.actor.clone();
        let book = f.book;
        let entry = f.chapters[2];
        async move {
            support::synchronize(
                &services,
                &actor,
                vec![Action::EntryRemove {
                    playlist_id: book,
                    entry_id: entry,
                }],
            )
            .await
        }
        .with_subscriber(dispatch)
    });
    tokio::time::timeout(Duration::from_secs(3), gate.deleting())
        .await
        .unwrap_or_else(|error| {
            panic!(
                "Owned Sync apply was not observed: {error}; finished={}",
                deleting.is_finished()
            )
        });
    let deletion_first = tokio::time::timeout(Duration::from_millis(150), &mut deleting)
        .await
        .is_ok();
    if deletion_first {
        assert_eq!(
            gate.order().last(),
            Some(&"deleted"),
            "The owned deletion must actually commit before publication is released"
        );
    }
    gate.release();
    let result = tokio::time::timeout(Duration::from_secs(3), submitting)
        .await
        .unwrap()
        .unwrap();
    if deletion_first {
        assert_eq!(
            result,
            Err(PlayerError::NotFound),
            "A chapter deleted before publication cannot be continued"
        );
        assert_eq!(*observed.borrow(), before);
        assert!(!observed.has_changed().unwrap());
    } else {
        let receipt = result.unwrap();
        observed.changed().await.unwrap();
        let committed = observed.borrow().clone();
        assert_eq!(committed.current.unwrap().chapter, Some(anchor));
        assert_eq!(committed.current_index, Some(2));
        assert_eq!(committed.queue_len, 3);
        assert_eq!(committed.speed, 1250);
        assert_eq!(player.submit(&f.actor, commit).await.unwrap(), receipt);
        tokio::time::timeout(Duration::from_secs(3), deleting)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            &gate.order()[1..],
            &["published", "deleted"],
            "The guard must retain authority until queue publication"
        );
    }
    assert_eq!(player.submit(&f.actor, old).await.unwrap(), old_receipt);
    let book = f
        .source
        .services
        .collection
        .audiobook(&f.actor, &f.book.to_string())
        .await
        .unwrap();
    assert!(
        !book
            .chapters
            .iter()
            .any(|chapter| chapter.entry_id == anchor.entry_id.to_string())
    );
    owner.close().await.unwrap();
    for recording in [a, b] {
        assert!(matches!(
            f.source
                .services
                .downloads
                .status(&f.actor, &recording.to_string())
                .await,
            Err(Error::NotFound)
        ));
    }
    assert_eq!(
        std::fs::read(f.source.music.join("track.wav")).unwrap(),
        f.source.original
    );
    assert_eq!(std::fs::read(f.second_path).unwrap(), f.second_original);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cancellation_settles_held_admission_without_blocking_new_commands_or_losing_accepted_receipts()
 {
    let f = audiobook::Library::start().await;
    let [a, b] = f.recordings;
    let target: AudiobookChapterAnchor = serde_json::from_value(f.anchors[2].clone()).unwrap();
    support::synchronize(
        &f.source.services,
        &f.actor,
        vec![
            Action::ProgressSet {
                recording_id: a,
                position_ms: 500,
                speed: 1.25,
            },
            Action::AudiobookChapterSet { chapter: target },
        ],
    )
    .await;
    let before_events = serde_json::to_value(
        f.source
            .services
            .collection
            .events(&f.actor, 0)
            .await
            .unwrap(),
    )
    .unwrap();
    let gate = admission_gate::AdmissionGate::default();
    let dispatch = tracing::Dispatch::new(tracing_subscriber::registry().with(gate.clone()));
    let (player, owner) =
        tracing::dispatcher::with_default(&dispatch, || Player::start(f.source.services.clone()));
    let attached = player
        .claim(&f.actor, player.inspect(&f.actor).await.unwrap().epoch)
        .await
        .unwrap();
    player
        .submit(
            &f.actor,
            PlayerCommand {
                id: Uuid::new_v4(),
                lease: attached.lease,
                sequence: 1,
                revision: 0,
                action: PlayerAction::Replace {
                    recordings: vec![b],
                    start: 0,
                },
            },
        )
        .await
        .unwrap();
    let observed = player.subscribe(&f.actor, attached.lease).await.unwrap();
    let before = observed.borrow().clone();
    let input = PlayerAudiobookRequest {
        id: Uuid::new_v4(),
        lease: attached.lease,
        revision: before.revision,
        selection: before.selection,
        audiobook_id: f.book,
    };
    let prepared = player
        .prepare_audiobook(&f.actor, input.clone())
        .await
        .unwrap();
    let command = PlayerCommand {
        id: Uuid::new_v4(),
        lease: attached.lease,
        sequence: 2,
        revision: before.revision,
        action: PlayerAction::ContinueAudiobook {
            token: prepared.token,
        },
    };
    let pool = sqlx::SqlitePool::connect_with(
        sqlx::sqlite::SqliteConnectOptions::new()
            .filename(f.source.directory.path().join("hito.db")),
    )
    .await
    .unwrap();
    let lock = pool.begin_with("BEGIN IMMEDIATE").await.unwrap();
    let submitting = tokio::spawn({
        let player = player.clone();
        let actor = f.actor.clone();
        let command = command.clone();
        async move { player.submit(&actor, command).await }
    });
    tokio::time::timeout(Duration::from_secs(1), gate.admitting())
        .await
        .unwrap();
    assert!(!submitting.is_finished());
    assert_eq!(*observed.borrow(), before);
    assert_eq!(
        tokio::time::timeout(
            Duration::from_secs(1),
            player.settle_audiobook(&f.actor, attached.lease, input.id, command.clone())
        )
        .await
        .unwrap()
        .unwrap(),
        None
    );
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(1), submitting)
            .await
            .unwrap()
            .unwrap(),
        Err(PlayerError::Conflict)
    );
    assert_eq!(*observed.borrow(), before);
    assert!(!observed.has_changed().unwrap());
    let next = player.submit(
        &f.actor,
        PlayerCommand {
            id: Uuid::new_v4(),
            lease: attached.lease,
            sequence: 3,
            revision: before.revision,
            action: PlayerAction::Replace {
                recordings: vec![b],
                start: 0,
            },
        },
    );
    let receipt = tokio::time::timeout(Duration::from_secs(1), next)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(receipt.revision, before.revision + 1);
    lock.rollback().await.unwrap();
    assert_eq!(
        player
            .settle_audiobook(&f.actor, attached.lease, input.id, command.clone())
            .await
            .unwrap(),
        None
    );
    assert_eq!(
        player.submit(&f.actor, command).await,
        Err(PlayerError::Conflict)
    );
    let current = player.inspect(&f.actor).await.unwrap().state.unwrap();
    let fresh = PlayerAudiobookRequest {
        id: Uuid::new_v4(),
        lease: attached.lease,
        revision: current.revision,
        selection: current.selection,
        audiobook_id: f.book,
    };
    let prepared = player
        .prepare_audiobook(&f.actor, fresh.clone())
        .await
        .unwrap();
    let command = PlayerCommand {
        id: Uuid::new_v4(),
        lease: attached.lease,
        sequence: 4,
        revision: current.revision,
        action: PlayerAction::ContinueAudiobook {
            token: prepared.token,
        },
    };
    let receipt = player.submit(&f.actor, command.clone()).await.unwrap();
    assert_eq!(
        player
            .settle_audiobook(&f.actor, attached.lease, fresh.id, command.clone())
            .await
            .unwrap(),
        Some(receipt.clone())
    );
    assert_eq!(
        player.submit(&f.actor, command.clone()).await.unwrap(),
        receipt
    );
    assert_eq!(
        player
            .settle_audiobook(&f.actor, attached.lease, Uuid::new_v4(), command)
            .await,
        Err(PlayerError::Conflict)
    );
    let committed = player.inspect(&f.actor).await.unwrap().state.unwrap();
    assert_eq!(committed.current.unwrap().chapter, Some(target));
    assert_eq!(committed.current_index, Some(2));
    assert_eq!(committed.speed, 1250);
    owner.close().await.unwrap();
    assert_eq!(
        serde_json::to_value(
            f.source
                .services
                .collection
                .events(&f.actor, 0)
                .await
                .unwrap()
        )
        .unwrap(),
        before_events
    );
    for recording in [a, b] {
        assert!(matches!(
            f.source
                .services
                .downloads
                .status(&f.actor, &recording.to_string())
                .await,
            Err(Error::NotFound)
        ));
    }
    assert_eq!(
        std::fs::read(f.source.music.join("track.wav")).unwrap(),
        f.source.original
    );
    assert_eq!(std::fs::read(f.second_path).unwrap(), f.second_original);
    pool.close().await;
}
