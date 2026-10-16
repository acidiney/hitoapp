#[path = "fixtures/playlist_service.rs"]
mod fixture;
mod support;

use hito_core::{
    Action, CollectionPublication, ImportProviderPlaylist, Operation, SyncBatch, SyncEvent,
};
use serde_json::json;
use sqlx::{Connection, SqliteConnection, sqlite::SqliteConnectOptions};
use std::{
    future::Future,
    pin::Pin,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    task::{Context, Poll, Wake, Waker},
    time::Duration,
};
use tokio::sync::Notify;
use tracing::{
    Event, Subscriber,
    field::{Field, Visit},
};
use tracing_subscriber::{Layer, layer::Context as LayerContext, prelude::*};
use uuid::Uuid;

#[tokio::test]
async fn cancelling_after_begin_ack_releases_the_writer_and_leaves_no_import_receipt() {
    let server = fixture::ProviderServer::start().await;
    let (dir, hito, actor, _shared, _music) = support::library("playlist-begin-cancel").await;
    let installed = fixture::connect(&hito, &actor, &server, &["playlists"]).await;
    fixture::playlist(&server, "playlist/cancel", "snapshot-one", vec![
        json!({"occurrence_id": "retained", "item": {"kind": "recording", "recording": {
            "id": "recording/retained", "title": "Retained recording", "artist": "Fixture artist"
        }}}),
    ]).await;
    let before_catalog = serde_json::to_value(
        hito.catalog
            .recordings(&actor, Default::default())
            .await
            .unwrap(),
    )
    .unwrap();
    let before_libraries =
        serde_json::to_value(hito.catalog.libraries(&actor).await.unwrap()).unwrap();
    assert_eq!(before_catalog["total"], 0);
    assert!(hito.collection.playlists(&actor).await.unwrap().is_empty());
    assert!(
        hito.collection
            .events(&actor, 0)
            .await
            .unwrap()
            .operations
            .is_empty()
    );
    let mut blocker = SqliteConnection::connect_with(
        &SqliteConnectOptions::new().filename(dir.path().join("hito.db")),
    )
    .await
    .unwrap();
    let blocking_transaction = blocker.begin_with("BEGIN IMMEDIATE").await.unwrap();
    let request_id = Uuid::new_v4();
    let request = || ImportProviderPlaylist {
        request_id,
        installation_revision: installed.revision,
        playlist_reference: "https://provider.invalid/begin-cancel-selection".into(),
    };
    let marker = PublicationStarted::default();
    let dispatch = tracing::Dispatch::new(tracing_subscriber::registry().with(marker.clone()));
    let preparation_wake = Arc::new(ImportWake::default());
    let mut importing = Box::pin(hito.collection.import_provider_playlist(
        &actor,
        &installed.id,
        request(),
    ));
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            let generation = preparation_wake.generation();
            assert!(
                poll_import(importing.as_mut(), &preparation_wake, &dispatch).is_pending(),
                "the held external writer must prevent publication from completing"
            );
            if marker.0.load(Ordering::SeqCst) {
                break;
            }
            preparation_wake.after(generation).await;
        }
    })
    .await
    .expect("the complete snapshot must reach the borrowed BEGIN with its connection acquired");
    assert_eq!(server.requests("v1/playlists/detail").await.len(), 2);
    assert_eq!(server.requests("v1/playlists/items").await.len(), 1);

    let begin_wake = Arc::new(ImportWake::default());
    assert!(
        poll_import(importing.as_mut(), &begin_wake, &dispatch).is_pending(),
        "a fresh waker must be registered while the actual BEGIN is still blocked"
    );
    let before_release = begin_wake.generation();
    assert_eq!(before_release, 0);
    blocking_transaction.rollback().await.unwrap();
    tokio::time::timeout(Duration::from_secs(3), begin_wake.after(before_release))
        .await
        .expect("the blocked BEGIN must complete and wake its fresh rendezvous receiver");
    assert!(
        poll_import(importing.as_mut(), &begin_wake, &dispatch).is_pending(),
        "acknowledged custom BEGIN must yield while locking its SQLite handle"
    );
    drop(importing);

    let recovery_playlist = Uuid::new_v4();
    let recovery_operation = Uuid::new_v4();
    tokio::time::timeout(
        Duration::from_secs(3),
        hito.collection.apply(
            &actor,
            SyncBatch {
                device_id: "begin-cancel-recovery".into(),
                operations: vec![Operation {
                    id: recovery_operation,
                    action: Action::PlaylistCreate {
                        playlist_id: recovery_playlist,
                        name: "Recovery playlist".into(),
                    },
                }],
            },
        ),
    )
    .await
    .expect("cancelled BEGIN must release SQLite for the next public collection write")
    .unwrap();
    assert_eq!(
        serde_json::to_value(
            hito.catalog
                .recordings(&actor, Default::default())
                .await
                .unwrap()
        )
        .unwrap(),
        before_catalog
    );
    assert_eq!(
        serde_json::to_value(hito.catalog.libraries(&actor).await.unwrap()).unwrap(),
        before_libraries
    );
    let playlists = hito.collection.playlists(&actor).await.unwrap();
    assert_eq!(playlists.len(), 1);
    assert_eq!(playlists[0].id, recovery_playlist.to_string());
    let feed = hito.collection.events(&actor, 0).await.unwrap();
    assert_eq!(feed.operations.len(), 1);
    match &feed.operations[0] {
        SyncEvent::Operation { operation, .. } => assert_eq!(operation.id, recovery_operation),
        SyncEvent::Publication { .. } => panic!("the cancelled import must publish no event"),
    }
    let accepted = tokio::time::timeout(
        Duration::from_secs(3),
        hito.collection
            .import_provider_playlist(&actor, &installed.id, request()),
    )
    .await
    .expect("the exact request must remain reusable after cancellation")
    .unwrap();
    assert_eq!(accepted.request_id, request_id);
    assert_ne!(accepted.playlist_id, recovery_playlist);
    let detail = hito
        .collection
        .playlist(&actor, &accepted.playlist_id.to_string())
        .await
        .unwrap();
    assert_eq!(detail.entries.len(), 1);
    assert_eq!(detail.entries[0].title, "Retained recording");
    assert!(!detail.entries[0].available);
    let recording = detail.entries[0].recording_id.as_deref().unwrap();
    assert!(
        hito.catalog
            .sources(&actor, recording)
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        hito.catalog
            .recordings(&actor, Default::default())
            .await
            .unwrap()
            .total,
        1
    );
    assert_eq!(hito.collection.playlists(&actor).await.unwrap().len(), 2);
    let feed = hito.collection.events(&actor, 0).await.unwrap();
    assert_eq!(feed.operations.len(), 2);
    assert!(!feed.has_more);
    match &feed.operations[1] {
        SyncEvent::Publication {
            publication: CollectionPublication::PlaylistImported { snapshot, .. },
            ..
        } => {
            assert_eq!(snapshot.playlist_id, accepted.playlist_id);
            assert_eq!(snapshot.entries.len(), 1);
            assert_eq!(snapshot.recordings.len(), 1);
        }
        SyncEvent::Operation { .. } => panic!("only the successful retry may publish the import"),
    }
    assert_eq!(server.requests("v1/playlists/detail").await.len(), 4);
    assert_eq!(server.requests("v1/playlists/items").await.len(), 2);
}

fn poll_import<F: Future>(
    future: Pin<&mut F>,
    wake: &Arc<ImportWake>,
    dispatch: &tracing::Dispatch,
) -> Poll<F::Output> {
    let waker = Waker::from(wake.clone());
    let mut context = Context::from_waker(&waker);
    tracing::dispatcher::with_default(dispatch, || future.poll(&mut context))
}

#[derive(Default)]
struct ImportWake {
    generation: AtomicUsize,
    changed: Notify,
}
impl ImportWake {
    fn generation(&self) -> usize {
        self.generation.load(Ordering::SeqCst)
    }
    async fn after(&self, generation: usize) {
        loop {
            let changed = self.changed.notified();
            if self.generation() > generation {
                return;
            }
            changed.await;
        }
    }
}
impl Wake for ImportWake {
    fn wake(self: Arc<Self>) {
        self.wake_by_ref();
    }
    fn wake_by_ref(self: &Arc<Self>) {
        self.generation.fetch_add(1, Ordering::SeqCst);
        self.changed.notify_one();
    }
}

#[derive(Clone, Default)]
struct PublicationStarted(Arc<AtomicBool>);
impl<S: Subscriber> Layer<S> for PublicationStarted {
    fn on_event(&self, event: &Event<'_>, _context: LayerContext<'_, S>) {
        event.record(&mut PublicationMarker(&self.0));
    }
}
struct PublicationMarker<'a>(&'a AtomicBool);
impl Visit for PublicationMarker<'_> {
    fn record_debug(&mut self, _field: &Field, _value: &dyn std::fmt::Debug) {}
    fn record_str(&mut self, field: &Field, value: &str) {
        if field.name() == "event" && value == "playlist_import_write_started" {
            self.0.store(true, Ordering::SeqCst);
        }
    }
}
