#[path = "fixtures/source_refresh.rs"]
mod fixture;
use hito_core::test_support::support;

use fixture::Fixture;
use futures_util::FutureExt;
use hito_core::{ResolveSource, SourceResolutionOutcome};
use serde_json::json;
use std::{panic::AssertUnwindSafe, sync::Arc, time::Duration};
use tokio::{
    sync::{Notify, oneshot},
    task::JoinHandle,
};
use tracing::{Subscriber, instrument::WithSubscriber};
use tracing_subscriber::{Layer, layer::Context, prelude::*};
use uuid::Uuid;

#[derive(Clone)]
struct Publication(Arc<Notify>);

impl<S: Subscriber> Layer<S> for Publication {
    fn on_event(&self, event: &tracing::Event<'_>, _: Context<'_, S>) {
        struct Marker(bool);
        impl tracing::field::Visit for Marker {
            fn record_debug(&mut self, _: &tracing::field::Field, _: &dyn std::fmt::Debug) {}
            fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
                if field.name() == "event" && value == "source_resolution_write_started" {
                    self.0 = true;
                }
            }
        }
        let mut marker = Marker(false);
        event.record(&mut marker);
        if marker.0 {
            self.0.notify_one();
        }
    }
}

#[tokio::test]
async fn started_automatic_publication_stays_owned_when_playback_is_cancelled() {
    let fixture = Fixture::start(json!({"items": []})).await;
    let selected = fixture
        .hito
        .catalog
        .recording_metadata(&fixture.actor, &fixture.recording.id, Default::default())
        .await
        .unwrap();
    let selected = serde_json::to_value(selected.selected_observation).unwrap();
    fixture.server.reply("v1/search", json!({"items": [{"id": "owned/audio-candidate", "title": "Accepted title", "artist": "Accepted artist", "duration_ms": 1000}], "next_cursor": null})).await;
    fixture.server.reply("v1/recordings/detail", json!({"id": "owned/audio-candidate", "title": "Accepted title", "artist": "Accepted artist", "duration_ms": 1000})).await;
    fixture.server.reply("v1/sources", json!({"items": [{"id": "owned/audio-source", "label": "Owned automatic source", "ready": true}]})).await;
    let pool = sqlx::SqlitePool::connect_with(
        sqlx::sqlite::SqliteConnectOptions::new()
            .filename(fixture.directory.path().join("hito.db"))
            .foreign_keys(true),
    )
    .await
    .unwrap();
    let details = fixture.server.hold("v1/recordings/detail").await;
    let publication = Publication(Arc::new(Notify::new()));
    hito_core::test_support::metadata_tag_logs::anchor_callsite_interest();
    let subscriber = tracing_subscriber::registry().with(publication.clone());
    let (cancel, cancelled) = oneshot::channel();
    let request_id = Uuid::new_v4();
    let mut pending = {
        let catalog = fixture.hito.catalog.clone();
        let actor = fixture.actor.clone();
        let recording = fixture.recording.id.clone();
        tokio::spawn(
            async move {
                catalog
                    .resolve_source_until(&actor, &recording, ResolveSource { request_id }, async {
                        let _ = cancelled.await;
                    })
                    .await
            }
            .with_subscriber(subscriber),
        )
    };
    let mut joined = false;
    let result = AssertUnwindSafe(async {
        tokio::time::timeout(Duration::from_secs(3), details.entered())
            .await
            .expect("the owned detail exchange must be reached");
        let transaction = pool.begin_with("BEGIN IMMEDIATE").await.unwrap();
        details.release();
        tokio::time::timeout(Duration::from_secs(3), publication.0.notified())
            .await
            .expect("the static publication marker must be observed before cancellation");
        cancel.send(()).unwrap();
        assert!(
            tokio::time::timeout(Duration::from_millis(50), &mut pending)
                .await
                .is_err(),
            "a started publication must remain owned while SQLite is busy"
        );
        transaction.rollback().await.unwrap();
        assert_published(&fixture, &mut pending, &mut joined, request_id, selected).await;
    })
    .catch_unwind()
    .await;
    if !joined {
        pending.abort();
        let _ = pending.await;
    }
    pool.close().await;
    drop(fixture);
    if let Err(panic) = result {
        std::panic::resume_unwind(panic);
    }
}

async fn assert_published(
    fixture: &Fixture,
    pending: &mut JoinHandle<hito_core::Result<hito_core::SourceResolution>>,
    joined: &mut bool,
    request_id: Uuid,
    selected: serde_json::Value,
) {
    let settled = tokio::time::timeout(Duration::from_secs(3), pending)
        .await
        .expect("owned publication must settle after the SQLite lock is released");
    *joined = true;
    let resolution = settled.unwrap().unwrap();
    assert!(
        matches!(resolution.outcome, SourceResolutionOutcome::Ready),
        "publication that already started must settle its accepted association before cancellation returns"
    );
    assert_eq!(resolution.request_id, request_id);
    assert!(resolution.recording_id == fixture.recording.id);
    assert_eq!(resolution.sources.len(), 1);
    assert!(resolution.sources[0].ready);
    let source_id = resolution.sources[0].id.clone();
    let metadata = fixture
        .hito
        .catalog
        .recording_metadata(&fixture.actor, &fixture.recording.id, Default::default())
        .await
        .unwrap();
    assert!(
        serde_json::to_value(metadata.selected_observation).unwrap() == selected,
        "automatic source publication must preserve primary metadata ownership"
    );
    let searches = fixture.server.requests("v1/search").await.len();
    assert_eq!(searches, 1);
    let replay = tokio::time::timeout(
        Duration::from_secs(1),
        fixture.hito.catalog.resolve_source_until(
            &fixture.actor,
            &fixture.recording.id,
            ResolveSource { request_id },
            std::future::pending(),
        ),
    )
    .await
    .expect("receipt replay must not wait for an abandoned lease")
    .unwrap();
    assert_eq!(replay.resolution_id, resolution.resolution_id);
    assert!(matches!(replay.outcome, SourceResolutionOutcome::Ready));
    assert!(
        replay
            .sources
            .iter()
            .any(|source| source.id == source_id && source.ready)
    );
    let next = tokio::time::timeout(
        Duration::from_secs(1),
        fixture.hito.catalog.resolve_source_until(
            &fixture.actor,
            &fixture.recording.id,
            ResolveSource {
                request_id: Uuid::new_v4(),
            },
            std::future::pending(),
        ),
    )
    .await
    .expect("the next explicit Play must not inherit a live cancelled claim")
    .unwrap();
    assert!(matches!(next.outcome, SourceResolutionOutcome::Ready));
    assert_ne!(next.resolution_id, resolution.resolution_id);
    assert!(
        next.sources
            .iter()
            .any(|source| source.id == source_id && source.ready)
    );
    assert_eq!(
        fixture.server.requests("v1/search").await.len(),
        searches,
        "ready sources and committed receipts must not repeat provider search"
    );
}
