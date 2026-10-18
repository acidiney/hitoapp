#[path = "fixtures/audiobook_queue.rs"]
mod audiobook;
#[path = "../../hito-core/tests/fixtures/playback.rs"]
mod fixture;
#[path = "../../hito-core/tests/support/mod.rs"]
mod support;

use hito_core::{Action, AudiobookChapterAnchor, Error};
use hito_playback::{Player, PlayerAction, PlayerAudiobookRequest, PlayerCommand};
use std::collections::HashSet;
use uuid::Uuid;

#[tokio::test]
async fn preparing_fresh_continuation_preserves_playback_until_one_exact_chapter_commit() {
    let audiobook::Library {
        source: f,
        actor,
        recordings: [a, b],
        book,
        chapters,
        anchors,
        second_path,
        second_original,
    } = audiobook::Library::start().await;
    let first: AudiobookChapterAnchor = serde_json::from_value(anchors[0].clone()).unwrap();
    let expected: AudiobookChapterAnchor = serde_json::from_value(anchors[2].clone()).unwrap();
    support::synchronize(
        &f.services,
        &actor,
        vec![
            Action::ProgressSet {
                recording_id: a,
                position_ms: 100,
                speed: 0.75,
            },
            Action::AudiobookChapterSet { chapter: first },
        ],
    )
    .await;
    let rendered = f
        .services
        .collection
        .audiobook(&actor, &book.to_string())
        .await
        .unwrap();
    assert_eq!(rendered.continuation.as_ref().unwrap().chapter, first);
    let (player, owner) = Player::start(f.services.clone());
    let attachment = player
        .claim(&actor, player.inspect(&actor).await.unwrap().epoch)
        .await
        .unwrap();
    let previous = PlayerCommand {
        id: Uuid::new_v4(),
        lease: attachment.lease,
        sequence: 1,
        revision: 0,
        action: PlayerAction::Replace {
            recordings: vec![b],
            start: 0,
        },
    };
    let previous_receipt = player.submit(&actor, previous.clone()).await.unwrap();
    let before_player = serde_json::to_value(player.inspect(&actor).await.unwrap()).unwrap();
    let before_queue = serde_json::to_value(
        player
            .queue(&actor, attachment.lease, 1, 0, 10)
            .await
            .unwrap(),
    )
    .unwrap();
    let mut observation = player.subscribe(&actor, attachment.lease).await.unwrap();
    support::synchronize(
        &f.services,
        &actor,
        vec![
            Action::ProgressSet {
                recording_id: a,
                position_ms: 500,
                speed: 1.25,
            },
            Action::AudiobookChapterSet { chapter: expected },
        ],
    )
    .await;
    let before_book = serde_json::to_value(
        f.services
            .collection
            .audiobook(&actor, &book.to_string())
            .await
            .unwrap(),
    )
    .unwrap();
    let before_events =
        serde_json::to_value(f.services.collection.events(&actor, 0).await.unwrap()).unwrap();
    let state = observation.borrow().clone();
    let request = PlayerAudiobookRequest {
        id: Uuid::new_v4(),
        lease: attachment.lease,
        revision: state.revision,
        selection: state.selection,
        audiobook_id: book,
    };
    let prepared = player
        .prepare_audiobook(&actor, request.clone())
        .await
        .unwrap();
    assert!(!prepared.token.is_nil());
    assert_ne!(prepared.token, request.id);
    assert_eq!(prepared.chapter, expected);
    assert_eq!(prepared.position_ms, 500);
    assert_eq!(prepared.speed, 1250);
    assert_eq!(prepared.count, 3);
    assert_eq!(
        serde_json::to_value(player.inspect(&actor).await.unwrap()).unwrap(),
        before_player
    );
    assert_eq!(
        serde_json::to_value(
            player
                .queue(&actor, attachment.lease, 1, 0, 10)
                .await
                .unwrap()
        )
        .unwrap(),
        before_queue
    );
    assert!(!observation.has_changed().unwrap());
    assert_eq!(
        player.submit(&actor, previous).await.unwrap(),
        previous_receipt,
        "Read-only preparation must preserve existing command receipts"
    );
    let commit = PlayerCommand {
        id: Uuid::new_v4(),
        lease: attachment.lease,
        sequence: 2,
        revision: 1,
        action: PlayerAction::ContinueAudiobook {
            token: prepared.token,
        },
    };
    let receipt = player.submit(&actor, commit.clone()).await.unwrap();
    assert_eq!((receipt.sequence, receipt.revision), (2, 2));
    observation.changed().await.unwrap();
    let committed = observation.borrow().clone();
    assert_eq!(committed.revision, 2);
    assert_eq!(committed.selection, state.selection + 1);
    assert_eq!(committed.queue_len, 3);
    assert_eq!(committed.current_index, Some(2));
    assert_eq!(committed.current.unwrap().chapter, Some(expected));
    assert_eq!(committed.current.unwrap().recording_id, a);
    assert_eq!(committed.speed, 1250);
    assert!(committed.desired_playing);
    let queue = player
        .queue(&actor, attachment.lease, 2, 0, 3)
        .await
        .unwrap();
    assert_eq!(
        queue
            .entries
            .iter()
            .map(|entry| entry.id)
            .collect::<HashSet<_>>()
            .len(),
        3
    );
    for (entry, anchor) in queue.entries.iter().zip(&anchors) {
        assert_eq!(
            serde_json::to_value(entry.chapter.unwrap()).unwrap(),
            *anchor
        );
        assert!(!chapters.contains(&entry.id));
    }
    assert_eq!(committed.current.unwrap().id, queue.entries[2].id);
    let after_player = serde_json::to_value(player.inspect(&actor).await.unwrap()).unwrap();
    assert_eq!(player.submit(&actor, commit).await.unwrap(), receipt);
    assert_eq!(
        serde_json::to_value(player.inspect(&actor).await.unwrap()).unwrap(),
        after_player
    );
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
        before_book
    );
    assert_eq!(
        serde_json::to_value(f.services.collection.events(&actor, 0).await.unwrap()).unwrap(),
        before_events,
        "Preparing and committing playback must not invent an observed chapter or listening checkpoint"
    );
    for recording in [a, b] {
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
