#![cfg(feature = "device")]
use hito_core::test_support::provider_audio as audio_fixture;
#[path = "../fixtures/retirement.rs"]
mod retirement;
use hito_playback::{Player, PlayerAction, PlayerCommand, PlayerError, PlayerPhase};
use std::time::Duration;
use uuid::Uuid;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires the isolated clocked audio monitor"]
async fn a_completed_old_refresh_cannot_start_output_before_current_retirement_joins() {
    let f = audio_fixture::AudioLibrary::start().await;
    f.server
        .audio(
            axum::http::StatusCode::BAD_GATEWAY,
            Default::default(),
            Vec::new(),
        )
        .await;
    let refresh = f.server.hold("v1/sources").await;
    let events = retirement::Events::default();
    let writer = events.clone();
    let dispatch = tracing::Dispatch::new(
        tracing_subscriber::fmt()
            .json()
            .with_ansi(false)
            .with_max_level(tracing::Level::DEBUG)
            .with_writer(move || writer.clone())
            .finish(),
    );
    let (player, owner) =
        tracing::dispatcher::with_default(&dispatch, || Player::native(f.hito.clone()));
    let attached = player
        .claim(&f.actor, player.inspect(&f.actor).await.unwrap().epoch)
        .await
        .unwrap();
    let mut state = player.subscribe(&f.actor, attached.lease).await.unwrap();
    let recording = Uuid::parse_str(&f.source.recording_id).unwrap();
    player
        .submit(
            &f.actor,
            PlayerCommand {
                id: Uuid::new_v4(),
                lease: attached.lease,
                sequence: 1,
                revision: 0,
                action: PlayerAction::Replace {
                    recordings: vec![recording, recording],
                    start: 0,
                },
            },
        )
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(3), refresh.entered())
        .await
        .unwrap();
    let pool = sqlx::SqlitePool::connect_with(
        sqlx::sqlite::SqliteConnectOptions::new().filename(f.directory.path().join("hito.db")),
    )
    .await
    .unwrap();
    let transaction = pool.begin_with("BEGIN IMMEDIATE").await.unwrap();
    refresh.open();
    tokio::time::timeout(Duration::from_secs(3), events.publication())
        .await
        .unwrap();
    let _retirement = events.arm();
    player
        .submit(
            &f.actor,
            PlayerCommand {
                id: Uuid::new_v4(),
                lease: attached.lease,
                sequence: 2,
                revision: 1,
                action: PlayerAction::Next {},
            },
        )
        .await
        .unwrap();
    tokio::time::timeout(
        Duration::from_secs(3),
        state.wait_for(|state| {
            state.selection == 2
                && state.phase == PlayerPhase::Resolving
                && state.error == Some(PlayerError::Playback)
        }),
    )
    .await
    .unwrap()
    .unwrap();
    f.ready().await;
    transaction.rollback().await.unwrap();
    tokio::time::timeout(Duration::from_secs(3), events.selected(3))
        .await
        .unwrap();
    let premature = tokio::time::timeout(Duration::from_millis(150), async {
        loop {
            if f.server.requests("v1/audio").await.len() > 2 {
                return;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .is_ok();
    events.release();
    assert!(
        !premature,
        "replacement input opened before the previous owner joined"
    );
    tokio::time::timeout(
        Duration::from_secs(3),
        state.wait_for(|state| state.selection == 2 && state.phase == PlayerPhase::Ready),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(f.server.requests("v1/audio").await.len(), 3);
    owner.close().await.unwrap();
    pool.close().await;
}
