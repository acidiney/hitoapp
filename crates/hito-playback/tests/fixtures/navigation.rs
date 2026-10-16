#![allow(dead_code)]
#[path = "recovery.rs"]
mod recovery;
use hito_playback::{
    Player, PlayerAction, PlayerCommand, PlayerOwner, PlayerPhase, PlayerPosition,
    PlayerQueueEntry, PlayerReceipt, PlayerSnapshot,
};
use std::time::Duration;
use tokio::sync::watch;
use uuid::Uuid;

pub struct Navigation {
    pub source: recovery::Recovery,
    pub player: Player,
    pub owner: PlayerOwner,
    pub lease: Uuid,
    pub entries: Vec<PlayerQueueEntry>,
    pub state: watch::Receiver<PlayerSnapshot>,
    pub position: watch::Receiver<PlayerPosition>,
    sequence: u64,
}
impl Navigation {
    pub async fn start(count: usize, start: usize) -> Self {
        let source = recovery::Recovery::start().await;
        let (player, owner) = Player::native(source.services.clone());
        let lease = player
            .claim(
                &source.actor,
                player.inspect(&source.actor).await.unwrap().epoch,
            )
            .await
            .unwrap()
            .lease;
        let state = player.subscribe(&source.actor, lease).await.unwrap();
        let position = player
            .subscribe_position(&source.actor, lease)
            .await
            .unwrap();
        let mut fixture = Self {
            source,
            player,
            owner,
            lease,
            entries: Vec::new(),
            state,
            position,
            sequence: 0,
        };
        fixture
            .send(PlayerAction::Replace {
                recordings: vec![Uuid::parse_str(&fixture.source.recording).unwrap(); count],
                start,
            })
            .await;
        fixture.entries = fixture
            .player
            .queue(&fixture.source.actor, lease, 1, 0, count)
            .await
            .unwrap()
            .entries;
        fixture
    }
    pub fn request(&self, action: PlayerAction) -> PlayerCommand {
        PlayerCommand {
            id: Uuid::new_v4(),
            lease: self.lease,
            sequence: self.sequence + 1,
            revision: self.state.borrow().revision,
            action,
        }
    }
    pub async fn send(&mut self, action: PlayerAction) -> PlayerReceipt {
        let receipt = self
            .player
            .submit(&self.source.actor, self.request(action))
            .await
            .unwrap();
        self.sequence = receipt.sequence;
        receipt
    }
    pub async fn playing(&mut self, entry: Uuid) {
        tokio::time::timeout(
            Duration::from_secs(3),
            self.state.wait_for(|state| {
                state.current.is_some_and(|current| current.id == entry)
                    && state.phase == PlayerPhase::Ready
                    && state.playing
            }),
        )
        .await
        .unwrap()
        .unwrap();
        tokio::time::timeout(
            Duration::from_secs(3),
            self.position
                .wait_for(|position| position.entry == Some(entry) && position.position_ms >= 100),
        )
        .await
        .unwrap()
        .unwrap();
    }
}
