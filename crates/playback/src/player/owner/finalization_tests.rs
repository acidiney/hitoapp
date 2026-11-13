use crate::player::{Player, PlayerAction, PlayerCommand, effects::Effects};
use hito_core::test_support::{hang_guard::hang_guard, provider_audio::AudioLibrary};
use std::time::Duration;
use tokio::sync::oneshot;
use uuid::Uuid;

#[tokio::test]
async fn held_finalizer_does_not_block_public_pause_or_replace_admission() {
    let library = hang_guard("owned held finalizer owner fixture", AudioLibrary::start()).await;
    let (entered, entered_rx) = oneshot::channel();
    let (release, release_rx) = oneshot::channel();
    let (handled, handled_rx) = oneshot::channel();
    let effects = Effects::native_with_held_finalizer(
        library.hito.catalog.clone(),
        library.hito.playback.clone(),
        entered,
        release_rx,
        handled,
    );
    let (player, owner) = Player::spawn(library.hito.clone(), effects, None);
    let attached = tokio::time::timeout(Duration::from_secs(3), async {
        let inspection = player.inspect(&library.actor).await.unwrap();
        player
            .claim(&library.actor, inspection.epoch)
            .await
            .unwrap()
    })
    .await
    .expect("claim must remain admissible while a finalizer is held");
    let recording = Uuid::parse_str(&library.source.recording_id).unwrap();
    let replaced = tokio::time::timeout(
        Duration::from_secs(3),
        player.submit(
            &library.actor,
            PlayerCommand {
                id: Uuid::new_v4(),
                lease: attached.lease,
                sequence: 1,
                revision: 0,
                action: PlayerAction::Replace {
                    recordings: vec![recording],
                    start: 0,
                },
            },
        ),
    )
    .await
    .expect("Replace must remain admissible while a finalizer is held")
    .unwrap();
    let resumed = tokio::time::timeout(
        Duration::from_secs(3),
        player.submit(
            &library.actor,
            PlayerCommand {
                id: Uuid::new_v4(),
                lease: attached.lease,
                sequence: 2,
                revision: replaced.revision,
                action: PlayerAction::Resume {},
            },
        ),
    )
    .await
    .expect("Resume must remain admissible before the matching finalizer is armed")
    .unwrap();
    tokio::time::timeout(Duration::from_secs(3), entered_rx)
        .await
        .expect("the finalizer task must arm for the selected playing target")
        .unwrap();
    let paused = tokio::time::timeout(
        Duration::from_secs(3),
        player.submit(
            &library.actor,
            PlayerCommand {
                id: Uuid::new_v4(),
                lease: attached.lease,
                sequence: 3,
                revision: resumed.revision,
                action: PlayerAction::Pause {},
            },
        ),
    )
    .await
    .expect("Pause must remain admissible while a finalizer is held")
    .unwrap();
    let successor = tokio::time::timeout(
        Duration::from_secs(3),
        player.submit(
            &library.actor,
            PlayerCommand {
                id: Uuid::new_v4(),
                lease: attached.lease,
                sequence: 4,
                revision: paused.revision,
                action: PlayerAction::Replace {
                    recordings: vec![recording],
                    start: 0,
                },
            },
        ),
    )
    .await
    .expect("successor Replace must remain admissible while a finalizer is held")
    .unwrap();
    let before_release = player.inspect(&library.actor).await.unwrap().state.unwrap();
    assert_eq!(successor.sequence, 4);
    assert_eq!(before_release.selection, 2);
    assert_eq!(before_release.current_index, Some(0));
    assert!(!before_release.desired_playing);

    release.send(()).unwrap();
    tokio::time::timeout(Duration::from_secs(3), handled_rx)
        .await
        .expect("the retired finalizer must be collected promptly after release")
        .unwrap();
    let after_release = player.inspect(&library.actor).await.unwrap().state.unwrap();
    assert_eq!(after_release.selection, before_release.selection);
    assert_eq!(after_release.current_index, before_release.current_index);
    assert_eq!(after_release.error, before_release.error);
    assert_eq!(after_release.last_failure, before_release.last_failure);
    let closed = owner.close().await.is_ok();
    drop(player);
    assert!(closed, "the public player owner must join cleanly");
}
