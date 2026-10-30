#![cfg(feature = "device")]
use hito_audio::test_support::capture;
use hito_core::test_support::provider_audio as audio_fixture;
use hito_playback::{Player, PlayerAction, PlayerCommand};
use std::{
    io::Write,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::sync::Notify;
use uuid::Uuid;

#[derive(Clone, Default)]
struct Events {
    bytes: Arc<Mutex<Vec<u8>>>,
    publication: Arc<Notify>,
    joined: Arc<Notify>,
}
impl Write for Events {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        let mut log = self.bytes.lock().unwrap();
        log.extend_from_slice(bytes);
        let text = String::from_utf8_lossy(&log);
        if text.contains("provider_refresh_write_started") {
            self.publication.notify_one();
        }
        if text.contains("player_owner_closed") {
            self.joined.notify_one();
        }
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires the isolated clocked audio monitor"]
async fn cancelling_close_observation_retains_admitted_source_publication_until_the_owner_joins() {
    let f = audio_fixture::AudioLibrary::start().await;
    f.server
        .audio(
            axum::http::StatusCode::BAD_GATEWAY,
            Default::default(),
            Vec::new(),
        )
        .await;
    let gate = f.server.hold("v1/sources").await;
    f.server
        .reply(
            "v1/sources",
            serde_json::json!({"items":[{"id":"replacement","label":"Original","ready":true}]}),
        )
        .await;
    let events = Events::default();
    let writer = events.clone();
    hito_testkit::logs::anchor_callsite_interest();
    let dispatch = tracing::Dispatch::new(
        tracing_subscriber::fmt()
            .json()
            .with_ansi(false)
            .with_max_level(tracing::Level::DEBUG)
            .with_writer(move || writer.clone())
            .finish(),
    );
    let capture = capture::Capture::open();
    let (player, owner) =
        tracing::dispatcher::with_default(&dispatch, || Player::native(f.hito.clone()));
    let attached = player
        .claim(&f.actor, player.inspect(&f.actor).await.unwrap().epoch)
        .await
        .unwrap();
    let mut state = player.subscribe(&f.actor, attached.lease).await.unwrap();
    player
        .submit(
            &f.actor,
            PlayerCommand {
                id: Uuid::new_v4(),
                lease: attached.lease,
                sequence: 1,
                revision: 0,
                action: PlayerAction::Replace {
                    recordings: vec![Uuid::parse_str(&f.source.recording_id).unwrap()],
                    start: 0,
                },
            },
        )
        .await
        .unwrap();
    player
        .submit(
            &f.actor,
            PlayerCommand {
                id: Uuid::new_v4(),
                lease: attached.lease,
                sequence: 2,
                revision: 1,
                action: PlayerAction::Resume {},
            },
        )
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(3), gate.entered())
        .await
        .unwrap();
    let pool = sqlx::SqlitePool::connect_with(
        sqlx::sqlite::SqliteConnectOptions::new().filename(f.directory.path().join("hito.db")),
    )
    .await
    .unwrap();
    let transaction = pool.begin_with("BEGIN IMMEDIATE").await.unwrap();
    gate.release();
    tokio::time::timeout(Duration::from_secs(3), events.publication.notified())
        .await
        .unwrap();
    let mut closing = tokio::spawn(owner.close());
    tokio::time::timeout(Duration::from_secs(2), async {
        while state.changed().await.is_ok() {}
    })
    .await
    .unwrap();
    assert!(
        tokio::time::timeout(Duration::from_millis(100), &mut closing)
            .await
            .is_err(),
        "close returned before the admitted write could finish"
    );
    closing.abort();
    assert!(closing.await.unwrap_err().is_cancelled());
    transaction.rollback().await.unwrap();
    tokio::time::timeout(Duration::from_secs(3), events.joined.notified())
        .await
        .unwrap();
    let sources = f
        .hito
        .catalog
        .sources(&f.actor, &f.source.recording_id)
        .await
        .unwrap();
    assert_eq!(sources.len(), 1);
    assert_ne!(sources[0].id, f.source.id);
    capture.after(capture.mark(), 0.1);
    assert!(capture::energy(&capture.recent(0.1)) < 0.000001);
    pool.close().await;
}
