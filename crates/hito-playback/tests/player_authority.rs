#[path = "fixtures/authority_outage.rs"]
mod authority_outage;
#[path = "../../hito-core/tests/fixtures/playback.rs"]
mod fixture;
#[path = "../../hito-core/tests/support/mod.rs"]
mod support;
use hito_playback::{Player, PlayerAction, PlayerCommand, PlayerError};
use std::time::Duration;
use uuid::Uuid;

#[tokio::test]
async fn temporary_authority_failure_preserves_the_queue_lease_and_accepted_receipts() {
    let f = fixture::LocalPlayback::start().await;
    let (player, owner) = Player::start(f.services.clone());
    let attached = player
        .claim(&f.actor, player.inspect(&f.actor).await.unwrap().epoch)
        .await
        .unwrap();
    let command = PlayerCommand {
        id: Uuid::new_v4(),
        lease: attached.lease,
        sequence: 1,
        revision: 0,
        action: PlayerAction::Replace {
            recordings: vec![Uuid::parse_str(&f.recording).unwrap(); 2],
            start: 0,
        },
    };
    let receipt = player.submit(&f.actor, command.clone()).await.unwrap();
    let page = player
        .queue(&f.actor, attached.lease, receipt.revision, 0, 10)
        .await
        .unwrap();
    let ids = page
        .entries
        .iter()
        .map(|entry| entry.id)
        .collect::<Vec<_>>();
    let mut state = player.subscribe(&f.actor, attached.lease).await.unwrap();
    let outage = authority_outage::Outage::start(f.directory.path()).await;
    tokio::time::timeout(
        Duration::from_secs(3),
        state.wait_for(|value| value.error == Some(PlayerError::Unavailable)),
    )
    .await
    .unwrap()
    .expect("temporary authority failure must keep the owned observation open");
    assert_eq!(state.borrow().queue_len, 2);
    assert_eq!(state.borrow().revision, receipt.revision);
    let next = PlayerCommand {
        id: Uuid::new_v4(),
        lease: attached.lease,
        sequence: 2,
        revision: receipt.revision,
        action: PlayerAction::Next {},
    };
    assert_eq!(
        player.submit(&f.actor, next.clone()).await,
        Err(PlayerError::Unavailable)
    );
    outage.restore().await;
    tokio::time::timeout(
        Duration::from_secs(3),
        state.wait_for(|value| value.error.is_none()),
    )
    .await
    .unwrap()
    .unwrap();
    let seen = player.inspect(&f.actor).await.unwrap();
    assert_eq!(seen.epoch, attached.epoch);
    assert_eq!(seen.state.unwrap().current.unwrap().id, ids[0]);
    assert_eq!(player.submit(&f.actor, command).await.unwrap(), receipt);
    let advanced = player.submit(&f.actor, next).await.unwrap();
    let after = player
        .queue(&f.actor, attached.lease, advanced.revision, 0, 10)
        .await
        .unwrap();
    assert_eq!(
        after
            .entries
            .iter()
            .map(|entry| entry.id)
            .collect::<Vec<_>>(),
        ids
    );
    assert_eq!(
        player
            .inspect(&f.actor)
            .await
            .unwrap()
            .state
            .unwrap()
            .current
            .unwrap()
            .id,
        ids[1]
    );
    owner.close().await.unwrap();
}
