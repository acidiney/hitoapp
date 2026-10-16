#![cfg(feature = "device")]
#[path = "fixtures/navigation.rs"]
mod fixture;
#[path = "fixtures/authority_outage.rs"]
mod outage;
#[path = "../../hito-core/tests/support/mod.rs"]
mod support;
use hito_playback::{PlayerAction, PlayerCommand, PlayerError, PlayerPhase, PlayerRepeat};
use std::time::Duration;
use uuid::Uuid;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires the isolated clocked audio monitor"]
async fn same_session_reattachment_preserves_history_and_another_login_cannot_recover_it() {
    let mut f = fixture::Navigation::start(3, 0).await;
    f.send(PlayerAction::Shuffle {}).await;
    f.send(PlayerAction::Repeat {}).await;
    f.send(PlayerAction::Speed { rate: 1500 }).await;
    f.send(PlayerAction::Resume {}).await;
    f.playing(f.entries[0].id).await;
    f.send(PlayerAction::Select {
        entry: f.entries[2].id,
    })
    .await;
    f.playing(f.entries[2].id).await;
    f.send(PlayerAction::Pause {}).await;
    let stale = f.request(PlayerAction::Previous {});
    let epoch = f.player.inspect(&f.source.actor).await.unwrap().epoch;
    let attached = f.player.claim(&f.source.actor, epoch).await.unwrap();
    assert_eq!(attached.state.current.unwrap().id, f.entries[2].id);
    assert!(attached.state.can_previous);
    assert!(attached.state.shuffle);
    assert_eq!(attached.state.repeat, PlayerRepeat::All);
    assert_eq!(attached.state.speed, 1500);
    for stale in [
        stale,
        f.request(PlayerAction::Shuffle {}),
        f.request(PlayerAction::Repeat {}),
        f.request(PlayerAction::Speed { rate: 2000 }),
    ] {
        assert_eq!(
            f.player.submit(&f.source.actor, stale).await,
            Err(PlayerError::Conflict)
        );
    }
    f.player
        .submit(
            &f.source.actor,
            PlayerCommand {
                id: Uuid::new_v4(),
                lease: attached.lease,
                sequence: 1,
                revision: attached.state.revision,
                action: PlayerAction::Previous {},
            },
        )
        .await
        .unwrap();
    let current = f
        .player
        .inspect(&f.source.actor)
        .await
        .unwrap()
        .state
        .unwrap();
    assert_eq!(current.current.unwrap().id, f.entries[0].id);
    assert!(!current.desired_playing);
    let login = f
        .source
        .services
        .identity
        .login(hito_core::Credentials {
            username: "admin".into(),
            password: "indexing test password".into(),
        })
        .await
        .unwrap();
    let another = f
        .source
        .services
        .identity
        .authenticate(&login.token)
        .await
        .unwrap();
    let inspected = f.player.inspect(&another).await.unwrap();
    assert!(inspected.state.is_none());
    let attached = f.player.claim(&another, inspected.epoch).await.unwrap();
    assert!(!attached.state.can_previous);
    assert!(!attached.state.shuffle);
    assert_eq!(attached.state.repeat, PlayerRepeat::Off);
    assert_eq!(attached.state.speed, 1000);
    assert_eq!(attached.state.queue_len, 0);
    f.player
        .submit(
            &another,
            PlayerCommand {
                id: Uuid::new_v4(),
                lease: attached.lease,
                sequence: 1,
                revision: 0,
                action: PlayerAction::Previous {},
            },
        )
        .await
        .unwrap();
    assert!(
        f.player
            .inspect(&another)
            .await
            .unwrap()
            .state
            .unwrap()
            .current
            .is_none()
    );
    f.owner.close().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires the isolated clocked audio monitor"]
async fn authority_suspension_rejects_previous_and_recovery_keeps_the_same_played_history() {
    let mut f = fixture::Navigation::start(3, 0).await;
    f.send(PlayerAction::Shuffle {}).await;
    f.send(PlayerAction::Repeat {}).await;
    f.send(PlayerAction::Speed { rate: 750 }).await;
    f.send(PlayerAction::Resume {}).await;
    f.playing(f.entries[0].id).await;
    f.send(PlayerAction::Select {
        entry: f.entries[2].id,
    })
    .await;
    f.playing(f.entries[2].id).await;
    let outage = outage::Outage::recordings(f.source.directory.path()).await;
    tokio::time::timeout(
        Duration::from_secs(3),
        f.state.wait_for(|s| s.phase == PlayerPhase::Suspended),
    )
    .await
    .unwrap()
    .unwrap();
    for action in [
        PlayerAction::Previous {},
        PlayerAction::Shuffle {},
        PlayerAction::Repeat {},
        PlayerAction::Speed { rate: 2000 },
    ] {
        assert_eq!(
            f.player.submit(&f.source.actor, f.request(action)).await,
            Err(PlayerError::Unavailable)
        );
    }
    assert!(f.state.borrow().shuffle);
    assert_eq!(f.state.borrow().repeat, PlayerRepeat::All);
    assert_eq!(f.state.borrow().speed, 750);
    assert_eq!(f.state.borrow().current.unwrap().id, f.entries[2].id);
    assert_eq!(f.state.borrow().revision, 2);
    outage.restore().await;
    tokio::time::timeout(
        Duration::from_secs(4),
        f.state
            .wait_for(|s| s.phase == PlayerPhase::Ready && s.playing),
    )
    .await
    .unwrap()
    .unwrap();
    let seek = f.send(PlayerAction::Seek { position_ms: 0 }).await;
    tokio::time::timeout(
        Duration::from_secs(3),
        f.position.wait_for(|p| p.seek_id == Some(seek.id)),
    )
    .await
    .unwrap()
    .unwrap();
    f.send(PlayerAction::Previous {}).await;
    assert_eq!(f.state.borrow().current.unwrap().id, f.entries[0].id);
    assert_eq!(f.state.borrow().revision, 3);
    f.owner.close().await.unwrap();
}
