#![cfg(feature = "device")]
#[path = "fixtures/navigation.rs"]
mod fixture;
#[path = "fixtures/saved_store.rs"]
mod saved_store;
#[path = "../../hito-core/tests/support/mod.rs"]
mod support;
use hito_core::Action;
use hito_playback::{PlayerAction, PlayerPhase};
use std::time::Duration;
use uuid::Uuid;

async fn saved(f: &fixture::Navigation, position_ms: i64) {
    support::synchronize(
        &f.source.services,
        &f.source.actor,
        vec![Action::ProgressSet {
            recording_id: Uuid::parse_str(&f.source.recording).unwrap(),
            position_ms,
            speed: 2.,
        }],
    )
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires the isolated clocked audio monitor"]
async fn explicit_forward_backward_and_zero_seeks_cancel_an_inflight_saved_read() {
    for position_ms in [7000, 500, 0] {
        let mut f = fixture::Navigation::start(1, 0).await;
        saved(&f, 6000).await;
        let store = saved_store::SavedStore::delayed(f.source.directory.path()).await;
        f.send(PlayerAction::ResumeSaved {}).await;
        f.send(PlayerAction::Pause {}).await;
        let seek = f.send(PlayerAction::Seek { position_ms }).await;
        tokio::time::timeout(
            Duration::from_secs(3),
            f.position.wait_for(|p| p.seek_id == Some(seek.id)),
        )
        .await
        .unwrap()
        .unwrap();
        store.restore().await;
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert_eq!(f.position.borrow().position_ms, position_ms);
        assert_eq!(f.position.borrow().seek_id, Some(seek.id));
        assert!(!f.state.borrow().playing);
        assert_eq!(f.state.borrow().error, None);
        f.owner.close().await.unwrap();
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires the isolated clocked audio monitor"]
async fn selecting_a_duplicate_occurrence_cancels_the_old_saved_start_without_restoring_it() {
    let mut f = fixture::Navigation::start(2, 0).await;
    saved(&f, 6000).await;
    let store = saved_store::SavedStore::delayed(f.source.directory.path()).await;
    f.send(PlayerAction::ResumeSaved {}).await;
    f.send(PlayerAction::Pause {}).await;
    f.send(PlayerAction::Select {
        entry: f.entries[1].id,
    })
    .await;
    tokio::time::timeout(
        Duration::from_secs(3),
        f.state.wait_for(|s| s.phase == PlayerPhase::Ready),
    )
    .await
    .unwrap()
    .unwrap();
    store.restore().await;
    assert_eq!(f.position.borrow().entry, Some(f.entries[1].id));
    assert_eq!(f.position.borrow().position_ms, 0);
    assert_eq!(f.position.borrow().seek_id, None);
    assert_eq!(
        f.source
            .services
            .collection
            .events(&f.source.actor, 0)
            .await
            .unwrap()
            .operations
            .len(),
        1
    );
    f.owner.close().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires the isolated clocked audio monitor"]
async fn same_session_reattachment_retains_the_fixed_writer_barrier_and_loading_intent() {
    let mut f = fixture::Navigation::start(1, 0).await;
    tokio::time::timeout(
        Duration::from_secs(3),
        f.state.wait_for(|s| s.phase == PlayerPhase::Ready),
    )
    .await
    .unwrap()
    .unwrap();
    let pool = sqlx::SqlitePool::connect_with(
        sqlx::sqlite::SqliteConnectOptions::new()
            .filename(f.source.directory.path().join("hito.db")),
    )
    .await
    .unwrap();
    let transaction = pool.begin_with("BEGIN IMMEDIATE").await.unwrap();
    let seek = f.send(PlayerAction::Seek { position_ms: 6000 }).await;
    tokio::time::timeout(
        Duration::from_secs(3),
        f.position.wait_for(|p| p.seek_id == Some(seek.id)),
    )
    .await
    .unwrap()
    .unwrap();
    f.send(PlayerAction::ResumeSaved {}).await;
    f.send(PlayerAction::Pause {}).await;
    f.reattach().await;
    assert_eq!(f.state.borrow().phase, PlayerPhase::Loading);
    transaction.rollback().await.unwrap();
    tokio::time::timeout(
        Duration::from_secs(3),
        f.state.wait_for(|s| s.phase == PlayerPhase::Ready),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(f.position.borrow().position_ms, 6000);
    assert_eq!(f.position.borrow().seek_id, None);
    assert!(!f.state.borrow().desired_playing);
    f.owner.close().await.unwrap();
    assert_eq!(
        f.source
            .services
            .collection
            .events(&f.source.actor, 0)
            .await
            .unwrap()
            .operations
            .len(),
        1
    );
    pool.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires the isolated clocked audio monitor"]
async fn takeover_clear_and_close_cancel_pending_reads_without_old_positions_or_errors() {
    for action in ["takeover", "clear", "close"] {
        let mut f = fixture::Navigation::start(1, 0).await;
        saved(&f, 6000).await;
        let store = saved_store::SavedStore::delayed(f.source.directory.path()).await;
        f.send(PlayerAction::ResumeSaved {}).await;
        if action == "takeover" {
            let session = f
                .source
                .services
                .identity
                .login(hito_core::Credentials {
                    username: "admin".into(),
                    password: "indexing test password".into(),
                })
                .await
                .unwrap();
            let actor = f
                .source
                .services
                .identity
                .authenticate(&session.token)
                .await
                .unwrap();
            let attached = f
                .player
                .claim(&actor, f.player.inspect(&actor).await.unwrap().epoch)
                .await
                .unwrap();
            assert_eq!(attached.state.queue_len, 0);
            let mut state = f.player.subscribe(&actor, attached.lease).await.unwrap();
            store.restore().await;
            assert!(
                tokio::time::timeout(Duration::from_millis(100), state.changed())
                    .await
                    .is_err()
            );
            assert_eq!(state.borrow().error, None);
            f.owner.close().await.unwrap();
        } else {
            if action == "clear" {
                f.send(PlayerAction::Clear {}).await;
            }
            tokio::time::timeout(Duration::from_secs(1), f.owner.close())
                .await
                .unwrap()
                .unwrap();
            tokio::time::timeout(Duration::from_secs(1), async {
                while f.state.changed().await.is_ok() {}
            })
            .await
            .unwrap();
            store.restore().await;
        }
    }
}
