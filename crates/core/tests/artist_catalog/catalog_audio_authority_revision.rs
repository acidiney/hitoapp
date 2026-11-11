use super::{
    artist_track_audio::{
        BONUS_OCCURRENCE, audio_provider, connect_audio, library::Album, request,
    },
    catalog_audio_rank_fallback::ranked,
};
use futures_util::FutureExt;
use hito_core::test_support::album_edition_service::AlbumProvider;
use std::{
    panic::{AssertUnwindSafe, resume_unwind},
    sync::{Mutex, mpsc},
    time::Duration,
};
use tokio::sync::oneshot;
use tracing::{
    Event, Subscriber,
    field::{Field, Visit},
    instrument::WithSubscriber,
};
use tracing_subscriber::{
    Layer,
    layer::{Context, SubscriberExt},
};

struct FinishedSearch {
    entered: Mutex<Option<oneshot::Sender<()>>>,
    release: Mutex<mpsc::Receiver<()>>,
}

#[derive(Default)]
struct SearchEvent(bool);
impl Visit for SearchEvent {
    fn record_str(&mut self, field: &Field, value: &str) {
        self.0 |= field.name() == "event" && value == "catalog_audio_search_finished";
    }
    fn record_debug(&mut self, _: &Field, _: &dyn std::fmt::Debug) {}
}

impl<S: Subscriber> Layer<S> for FinishedSearch {
    fn on_event(&self, event: &Event<'_>, _: Context<'_, S>) {
        let mut visitor = SearchEvent::default();
        event.record(&mut visitor);
        if visitor.0
            && let Some(entered) = self.entered.lock().unwrap().take()
        {
            let _ = entered.send(());
            let _ = self
                .release
                .lock()
                .unwrap()
                .recv_timeout(Duration::from_secs(5));
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn changing_service_authority_after_hydration_refuses_old_candidate_provenance() {
    let mut catalog = AlbumProvider::start().await;
    let audio = audio_provider::ProviderServer::start().await;
    let album = Album::prepare(&catalog).await;
    let installation = connect_audio(&album, &audio).await;
    ranked(&audio, &[("audio/first", "Other title", true)]).await;
    let before = album.library().await;
    let (entered, wait) = oneshot::channel();
    let (release, released) = mpsc::channel();
    let subscriber = tracing_subscriber::registry().with(FinishedSearch {
        entered: Mutex::new(Some(entered)),
        release: Mutex::new(released),
    });
    let capability = album.hito.artist_catalog.clone();
    let actor = album.actor.clone();
    let recording = album.first.clone();
    let association = album.association.id.clone();
    let mut searching = Some(tokio::spawn(
        async move {
            capability
                .find_artist_track_audio_until(
                    &actor,
                    &recording,
                    &association,
                    request(BONUS_OCCURRENCE),
                    std::future::pending(),
                )
                .await
        }
        .with_subscriber(subscriber),
    ));
    let result = AssertUnwindSafe(async {
        tokio::time::timeout(Duration::from_secs(5), wait)
            .await
            .unwrap()
            .unwrap();
        album
            .sql(
                "UPDATE plugin_services SET revision = revision + 1 WHERE id = ?",
                &[&installation.service_id],
            )
            .await;
        release.send(()).unwrap();
        let outcome = tokio::time::timeout(Duration::from_secs(10), searching.as_mut().unwrap())
            .await
            .unwrap()
            .unwrap();
        searching.take();
        let observed = outcome
            .as_ref()
            .map(|result| result.outcome())
            .unwrap_or_else(|error| error.code());
        assert_eq!(
            observed, "conflict",
            "An operation opened after the service changed must not publish old candidate evidence"
        );
        assert_eq!(album.library().await, before);
        assert_eq!(audio.requests("v1/search").await.len(), 1);
        assert_eq!(audio.requests("v1/recordings/metadata").await.len(), 1);
    })
    .catch_unwind()
    .await;
    let _ = release.send(());
    if let Some(searching) = searching {
        searching.abort();
        let _ = searching.await;
    }
    audio.shutdown().await;
    let joined = catalog.finish().await;
    if let Err(panic) = result {
        resume_unwind(panic);
    }
    assert!(joined);
}
