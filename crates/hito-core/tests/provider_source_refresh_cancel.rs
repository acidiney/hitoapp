#[path = "fixtures/source_refresh.rs"]
mod fixture;
mod support;
use fixture::Fixture;
use serde_json::json;
use std::time::Duration;

#[derive(Clone)]
struct Publication(std::sync::Arc<(std::sync::Mutex<Vec<u8>>, tokio::sync::Notify, String)>);
impl std::io::Write for Publication {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        let mut log = self.0.0.lock().unwrap();
        log.extend_from_slice(bytes);
        if String::from_utf8_lossy(&log)
            .lines()
            .any(|line| line.contains("provider_refresh_write_started") && line.contains(&self.0.2))
        {
            self.0.1.notify_one();
        }
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for Publication {
    type Writer = Self;
    fn make_writer(&'a self) -> Self {
        self.clone()
    }
}

#[tokio::test]
async fn cancellation_after_publication_starts_keeps_the_transaction_owned_until_completion() {
    let f = Fixture::start(
        json!({"items":[{"id":"original/audio", "label":"Original", "ready":true}]}),
    )
    .await;
    let before = f
        .hito
        .catalog
        .sources(&f.actor, &f.recording.id)
        .await
        .unwrap();
    f.server
        .reply(
            "v1/sources",
            json!({"items":[{"id":"replacement/audio", "label":"Replacement", "ready":true}]}),
        )
        .await;
    let pool = sqlx::SqlitePool::connect_with(
        sqlx::sqlite::SqliteConnectOptions::new().filename(f.directory.path().join("hito.db")),
    )
    .await
    .unwrap();
    let gate = f.server.hold("v1/sources").await;
    let publication = Publication(std::sync::Arc::new((
        Default::default(),
        Default::default(),
        f.actor.user().id.clone(),
    )));
    let subscriber = tracing_subscriber::fmt()
        .with_env_filter("hito_core=debug")
        .with_ansi(false)
        .without_time()
        .with_writer(publication.clone())
        .finish();
    tracing::subscriber::set_global_default(subscriber).unwrap();
    let (cancel, cancelled) = tokio::sync::oneshot::channel::<()>();
    let mut pending = {
        let catalog = f.hito.catalog.clone();
        let actor = f.actor.clone();
        let id = f.recording.id.clone();
        tokio::spawn(async move {
            catalog
                .refresh_sources_until(&actor, &id, async {
                    let _ = cancelled.await;
                })
                .await
        })
    };
    tokio::time::timeout(Duration::from_secs(3), gate.entered())
        .await
        .unwrap();
    let transaction = pool.begin_with("BEGIN IMMEDIATE").await.unwrap();
    gate.release();
    if tokio::time::timeout(Duration::from_secs(3), publication.0.1.notified())
        .await
        .is_err()
    {
        let log = publication.0.0.lock().unwrap();
        let log = String::from_utf8_lossy(&log);
        panic!(
            "publication not observed: finished={}, log_bytes={}, marker={}, core={}, sql={}, reqwest={}, response={}",
            pending.is_finished(),
            log.len(),
            log.contains("provider_refresh_write_started"),
            log.contains("hito_core"),
            log.contains("sqlx"),
            log.contains("reqwest"),
            log.contains("Plugin response received")
        );
    }
    cancel.send(()).unwrap();
    assert!(
        tokio::time::timeout(Duration::from_millis(50), &mut pending)
            .await
            .is_err(),
        "a started publication must remain owned while SQLite is busy"
    );
    transaction.rollback().await.unwrap();
    let after = tokio::time::timeout(Duration::from_secs(3), pending)
        .await
        .unwrap()
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(after.len(), 1);
    assert_ne!(after[0].id, before[0].id);
    assert!(after[0].ready);
    pool.close().await;
}

#[tokio::test]
async fn cancelling_a_pending_provider_refresh_preserves_sources_and_frees_admission() {
    let f = Fixture::start(
        json!({"items":[{"id":"original/audio", "label":"Original", "ready":true}]}),
    )
    .await;
    let before = f
        .hito
        .catalog
        .sources(&f.actor, &f.recording.id)
        .await
        .unwrap();
    f.server
        .reply(
            "v1/sources",
            json!({"items":[{"id":"replacement/audio", "label":"Replacement", "ready":true}]}),
        )
        .await;
    let gate = f.server.hold("v1/sources").await;
    let (cancel, cancelled) = tokio::sync::oneshot::channel::<()>();
    let pending = {
        let catalog = f.hito.catalog.clone();
        let actor = f.actor.clone();
        let id = f.recording.id.clone();
        tokio::spawn(async move {
            catalog
                .refresh_sources_until(&actor, &id, async {
                    let _ = cancelled.await;
                })
                .await
        })
    };
    tokio::time::timeout(Duration::from_secs(3), gate.entered())
        .await
        .unwrap();
    cancel.send(()).unwrap();
    assert!(
        tokio::time::timeout(Duration::from_secs(1), pending)
            .await
            .expect("cancellation must not wait for the slow provider")
            .unwrap()
            .unwrap()
            .is_none()
    );
    let unchanged = f
        .hito
        .catalog
        .sources(&f.actor, &f.recording.id)
        .await
        .unwrap();
    assert_eq!(unchanged.len(), 1);
    assert_eq!(unchanged[0].id, before[0].id);
    assert!(unchanged[0].ready);
    let replacement = {
        let catalog = f.hito.catalog.clone();
        let actor = f.actor.clone();
        let id = f.recording.id.clone();
        tokio::spawn(async move { catalog.refresh_sources(&actor, &id).await })
    };
    tokio::time::timeout(Duration::from_secs(3), gate.entered())
        .await
        .expect("cancelled work must release provider admission");
    gate.release();
    gate.release();
    let replacement = tokio::time::timeout(Duration::from_secs(3), replacement)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(replacement.len(), 1);
    assert_ne!(replacement[0].id, before[0].id);
}
