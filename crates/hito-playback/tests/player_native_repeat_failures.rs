#![cfg(feature = "device")]
#[path = "../../hito-core/tests/fixtures/provider_audio.rs"]
mod audio_fixture;
#[path = "../../hito-core/tests/fixtures/content_service.rs"]
mod fixture;
#[path = "../../hito-core/tests/support/mod.rs"]
mod support;
use hito_playback::{Player, PlayerAction, PlayerCommand};
use std::time::Duration;
use uuid::Uuid;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires the isolated clocked audio monitor"]
async fn repeat_modes_cannot_loop_an_unavailable_queue_and_repeat_all_checks_the_wrapped_entries() {
    for (cycles, shuffled) in [(1, false), (2, false), (1, true), (2, true)] {
        let f = audio_fixture::AudioLibrary::start().await;
        f.server
            .audio(
                axum::http::StatusCode::BAD_GATEWAY,
                Default::default(),
                Vec::new(),
            )
            .await;
        let (player, owner) = Player::native(f.hito.clone());
        let attached = player
            .claim(&f.actor, player.inspect(&f.actor).await.unwrap().epoch)
            .await
            .unwrap();
        let mut state = player.subscribe(&f.actor, attached.lease).await.unwrap();
        let mut actions = vec![PlayerAction::Replace {
            recordings: vec![Uuid::parse_str(&f.source.recording_id).unwrap(); 2],
            start: usize::from(cycles == 1),
        }];
        if shuffled {
            actions.push(PlayerAction::Shuffle {});
        }
        actions.extend(std::iter::repeat_n(PlayerAction::Repeat {}, cycles));
        actions.push(PlayerAction::Resume {});
        for (sequence, action) in actions.into_iter().enumerate() {
            let revision = state.borrow().revision;
            player
                .submit(
                    &f.actor,
                    PlayerCommand {
                        id: Uuid::new_v4(),
                        lease: attached.lease,
                        sequence: sequence as u64 + 1,
                        revision,
                        action,
                    },
                )
                .await
                .unwrap();
        }
        let ended = tokio::time::timeout(
            Duration::from_secs(4),
            state.wait_for(|state| state.current.is_none()),
        )
        .await
        .is_ok();
        owner.close().await.unwrap();
        assert!(
            ended,
            "An unavailable queue must finish even with repeat enabled"
        );
        assert!(!state.borrow().desired_playing);
        assert_eq!(f.server.requests("v1/sources").await.len(), 3);
        assert_eq!(f.server.requests("v1/audio").await.len(), 4);
    }
}
