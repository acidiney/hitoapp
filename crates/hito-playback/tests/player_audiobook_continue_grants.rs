#[path = "../../hito-core/tests/fixtures/audiobook_continue_library.rs"]
mod library;
#[path = "../../hito-core/tests/fixtures/content_service.rs"]
mod provider;
#[path = "../../hito-core/tests/support/mod.rs"]
mod support;

use hito_core::{Action, AudiobookChapterAnchor, Error};
use hito_playback::{Player, PlayerAction, PlayerAudiobookRequest, PlayerCommand};
use std::collections::HashSet;
use uuid::Uuid;

#[tokio::test]
async fn native_continue_retains_authorized_unready_chapters_without_an_unrelated_denied_recording()
{
    let f = library::Library::start().await;
    let expected = AudiobookChapterAnchor {
        audiobook_id: f.book,
        entry_id: f.entries[3],
        recording_id: f.recordings[0],
    };
    support::synchronize(
        &f.services,
        &f.actor,
        vec![
            Action::ProgressSet {
                recording_id: f.recordings[0],
                position_ms: 750,
                speed: 0.75,
            },
            Action::AudiobookChapterSet { chapter: expected },
        ],
    )
    .await;
    let before = f.snapshot().await;
    let (player, owner) = Player::start(f.services.clone());
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
                    recordings: vec![f.recordings[2]],
                    start: 0,
                },
            },
        )
        .await
        .unwrap();
    let state = player.inspect(&f.actor).await.unwrap().state.unwrap();
    let old_queue =
        serde_json::to_value(player.queue(&f.actor, lease, 1, 0, 10).await.unwrap()).unwrap();
    let prepared = player.prepare_audiobook(&f.actor, PlayerAudiobookRequest {
        id: Uuid::new_v4(), lease, revision: state.revision, selection: state.selection,
        audiobook_id: f.book,
    }).await.expect("A denied unrelated chapter must not prevent native continuation of the exact authorized chapter");
    assert_eq!(prepared.chapter, expected);
    assert_eq!(prepared.position_ms, 750);
    assert_eq!(prepared.speed, 750);
    assert_eq!(prepared.count, 3);
    assert_eq!(
        player.inspect(&f.actor).await.unwrap().state.unwrap(),
        state
    );
    assert_eq!(
        serde_json::to_value(player.queue(&f.actor, lease, 1, 0, 10).await.unwrap()).unwrap(),
        old_queue
    );
    let receipt = player
        .submit(
            &f.actor,
            PlayerCommand {
                id: Uuid::new_v4(),
                lease,
                sequence: 2,
                revision: 1,
                action: PlayerAction::ContinueAudiobook {
                    token: prepared.token,
                },
            },
        )
        .await
        .unwrap();
    assert_eq!(receipt.revision, 2);
    let committed = player.inspect(&f.actor).await.unwrap().state.unwrap();
    assert_eq!(committed.current_index, Some(2));
    assert_eq!(committed.current.unwrap().chapter, Some(expected));
    assert_eq!(committed.speed, 750);
    assert!(committed.desired_playing);
    let queue = player.queue(&f.actor, lease, 2, 0, 10).await.unwrap();
    assert_eq!(queue.total, 3);
    assert_eq!(
        queue
            .entries
            .iter()
            .map(|entry| entry.id)
            .collect::<HashSet<_>>()
            .len(),
        3
    );
    for (entry, index) in queue.entries.iter().zip([0, 2, 3]) {
        assert_eq!(entry.chapter.unwrap().entry_id, f.entries[index]);
        assert_eq!(entry.chapter.unwrap().audiobook_id, f.book);
        assert!(!f.entries.contains(&entry.id));
        assert_ne!(entry.recording.id, f.recordings[1].to_string());
    }
    assert_eq!(queue.entries[0].recording.id, f.recordings[0].to_string());
    assert_eq!(queue.entries[1].recording.id, f.recordings[2].to_string());
    assert!(!queue.entries[1].recording.available && queue.entries[1].recording.liked);
    assert_eq!(queue.entries[2].recording.id, f.recordings[0].to_string());
    assert_eq!(committed.current.unwrap().id, queue.entries[2].id);
    assert_ne!(queue.entries[0].id, queue.entries[2].id);
    assert!(player.inspect(&f.admin).await.unwrap().state.is_none());
    owner.close().await.unwrap();
    assert_eq!(
        f.snapshot().await,
        before,
        "Continue admission must preserve private musical data, source overrides, events and stored continuation until audio is observed"
    );
    for recording in f.recordings {
        assert!(matches!(
            f.services
                .downloads
                .status(&f.actor, &recording.to_string())
                .await,
            Err(Error::NotFound)
        ));
    }
    assert_eq!(std::fs::read(&f.original_path).unwrap(), f.original);
    assert!(f.server.requests("v1/audio").await.is_empty());
    assert!(f.directory.path().join("hito.db").is_file());
}
