#[path = "../../hito-core/tests/fixtures/playback.rs"]
mod fixture;
#[path = "../../hito-core/tests/support/mod.rs"]
mod support;
use hito_core::{Action, Credentials};
use hito_playback::{Player, PlayerAction, PlayerCommand, PlayerError};
use uuid::Uuid;

#[tokio::test]
async fn switching_accounts_closes_old_observations_and_never_reuses_private_queue_metadata() {
    let f = fixture::LocalPlayback::start().await;
    let recording = Uuid::parse_str(&f.recording).unwrap();
    support::synchronize(
        &f.services,
        &f.actor,
        vec![Action::RecordingTitleSet {
            recording_id: recording,
            title: "Private first-account queue title".into(),
        }],
    )
    .await;
    let user = f
        .services
        .identity
        .create_user(
            &f.actor,
            Credentials {
                username: "player-listener".into(),
                password: "private player listener password".into(),
            },
        )
        .await
        .unwrap();
    let session = f
        .services
        .identity
        .login(Credentials {
            username: user.username,
            password: "private player listener password".into(),
        })
        .await
        .unwrap();
    let another = f
        .services
        .identity
        .authenticate(&session.token)
        .await
        .unwrap();
    let (player, owner) = Player::start(f.services.clone());
    let attached = player
        .claim(&f.actor, player.inspect(&f.actor).await.unwrap().epoch)
        .await
        .unwrap();
    let mut old = player.subscribe(&f.actor, attached.lease).await.unwrap();
    player
        .submit(
            &f.actor,
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
        )
        .await
        .unwrap();
    assert_eq!(
        old.borrow_and_update().current.unwrap().recording_id,
        recording
    );
    assert_eq!(
        player
            .queue(&f.actor, attached.lease, 1, 0, 10)
            .await
            .unwrap()
            .entries[0]
            .recording
            .title,
        "Private first-account queue title"
    );
    let seen = player.inspect(&another).await.unwrap();
    assert!(seen.state.is_none());
    let next = player.claim(&another, seen.epoch).await.unwrap();
    assert_eq!(next.state.queue_len, 0);
    assert!(next.state.current.is_none());
    assert!(old.changed().await.is_err());
    assert!(matches!(
        player.queue(&f.actor, attached.lease, 1, 0, 10).await,
        Err(PlayerError::Conflict)
    ));
    assert!(matches!(
        player
            .submit(
                &another,
                PlayerCommand {
                    id: Uuid::new_v4(),
                    lease: next.lease,
                    sequence: 1,
                    revision: 0,
                    action: PlayerAction::Replace {
                        recordings: vec![recording],
                        start: 0
                    },
                }
            )
            .await,
        Err(PlayerError::NotFound)
    ));
    assert_eq!(
        player
            .inspect(&another)
            .await
            .unwrap()
            .state
            .unwrap()
            .revision,
        0
    );
    f.services
        .identity
        .set_library_access(&f.actor, &f.library, &user.id, true)
        .await
        .unwrap();
    player
        .submit(
            &another,
            PlayerCommand {
                id: Uuid::new_v4(),
                lease: next.lease,
                sequence: 2,
                revision: 0,
                action: PlayerAction::Replace {
                    recordings: vec![recording],
                    start: 0,
                },
            },
        )
        .await
        .unwrap();
    assert_ne!(
        player
            .queue(&another, next.lease, 1, 0, 10)
            .await
            .unwrap()
            .entries[0]
            .recording
            .title,
        "Private first-account queue title"
    );
    assert!(player.inspect(&f.actor).await.unwrap().state.is_none());
    owner.close().await.unwrap();
}

#[tokio::test]
async fn logout_rejects_retained_actors_and_closes_subscriptions_without_another_command() {
    let f = fixture::LocalPlayback::start().await;
    let (player, owner) = Player::start(f.services.clone());
    let attached = player
        .claim(&f.actor, player.inspect(&f.actor).await.unwrap().epoch)
        .await
        .unwrap();
    let mut observed = player.subscribe(&f.actor, attached.lease).await.unwrap();
    f.services.identity.logout(&f.token).await.unwrap();
    assert!(matches!(
        player.inspect(&f.actor).await,
        Err(PlayerError::Unauthorized)
    ));
    assert!(matches!(
        player.claim(&f.actor, attached.epoch).await,
        Err(PlayerError::Unauthorized)
    ));
    assert!(matches!(
        player.subscribe(&f.actor, attached.lease).await,
        Err(PlayerError::Unauthorized)
    ));
    tokio::time::timeout(std::time::Duration::from_secs(2), observed.changed())
        .await
        .unwrap()
        .unwrap_err();
    owner.close().await.unwrap();
}

#[tokio::test]
async fn queue_pages_recheck_library_access_instead_of_returning_cached_metadata() {
    let f = fixture::LocalPlayback::start().await;
    let (player, owner) = Player::start(f.services.clone());
    let lease = player
        .claim(&f.actor, player.inspect(&f.actor).await.unwrap().epoch)
        .await
        .unwrap()
        .lease;
    let command = PlayerCommand {
        id: Uuid::new_v4(),
        lease,
        sequence: 1,
        revision: 0,
        action: PlayerAction::Replace {
            recordings: vec![Uuid::parse_str(&f.recording).unwrap()],
            start: 0,
        },
    };
    let receipt = player.submit(&f.actor, command.clone()).await.unwrap();
    assert_eq!(
        player
            .queue(&f.actor, lease, 1, 0, 10)
            .await
            .unwrap()
            .entries
            .len(),
        1
    );
    f.services
        .identity
        .set_library_access(&f.actor, &f.library, &f.actor.user().id, false)
        .await
        .unwrap();
    assert!(matches!(
        player.queue(&f.actor, lease, 1, 0, 10).await,
        Err(PlayerError::NotFound)
    ));
    assert_eq!(player.submit(&f.actor, command).await.unwrap(), receipt);
    owner.close().await.unwrap();
}
