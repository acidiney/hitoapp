use crate::{RuntimeConfig, config::RuntimeResult, router_with_config};
use std::{
    future::Future,
    time::{Duration, Instant},
};
use tracing::{Instrument, instrument::WithSubscriber};

pub async fn run(
    config: RuntimeConfig,
    shutdown: impl Future<Output = ()> + Send + 'static,
) -> RuntimeResult<()> {
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| "hito_server=info,hito_core=info".into());
    if config.json_logs {
        tracing_subscriber::fmt()
            .with_env_filter(filter)
            .with_span_events(tracing_subscriber::fmt::format::FmtSpan::CLOSE)
            .json()
            .try_init()?;
    } else {
        tracing_subscriber::fmt()
            .with_env_filter(filter)
            .with_span_events(tracing_subscriber::fmt::format::FmtSpan::CLOSE)
            .try_init()?;
    }
    tracing::info!(event = "server_starting", "Starting Hito");
    let services = hito_core::open(config.core).await?;
    let listener = tokio::net::TcpListener::bind(config.listen).await?;
    serve(
        listener,
        router_with_config(services.clone(), config.http),
        services,
        shutdown,
    )
    .await
}

pub async fn serve(
    listener: tokio::net::TcpListener,
    app: axum::Router,
    services: hito_core::Services,
    shutdown: impl Future<Output = ()> + Send + 'static,
) -> RuntimeResult<()> {
    let (cancel, cancelled) = tokio::sync::oneshot::channel::<()>();
    let serving = tokio::spawn(
        serve_owned(listener, app, services, async move {
            tokio::select! {
                _ = shutdown => {},
                _ = cancelled => {},
            }
        })
        .instrument(tracing::Span::current())
        .with_current_subscriber(),
    );
    let result = serving.await?;
    drop(cancel);
    result
}

async fn serve_owned(
    listener: tokio::net::TcpListener,
    app: axum::Router,
    services: hito_core::Services,
    shutdown: impl Future<Output = ()> + Send + 'static,
) -> RuntimeResult<()> {
    let (stop, stopped) = tokio::sync::watch::channel(false);
    let indexing = indexing_worker(services.storage.clone(), stopped.clone());
    let artwork = artwork_worker(services.catalog, stopped.clone());
    let follows = follow_worker(services.collection, stopped);
    let http = crate::connections::serve(listener, app, stop, shutdown);
    let (result, (), (), ()) = tokio::join!(http, indexing, artwork, follows);
    services.storage.finish_indexing().await?;
    tracing::info!(event = "server_stopped", "Hito stopped");
    result
}

async fn indexing_worker(
    indexer: hito_core::LibraryStorage,
    mut stopped: tokio::sync::watch::Receiver<bool>,
) {
    tracing::info!(event = "worker_started", "Indexing worker started");
    let mut interval = tokio::time::interval(Duration::from_secs(1));
    loop {
        tokio::select! {
            biased;
            _ = stopped.changed() => break,
            _ = interval.tick() => {}
        }
        if let Err(error) = indexer
            .run_next_job_until(async {
                let _ = stopped.changed().await;
            })
            .await
        {
            tracing::error!(
                event = "worker_failed",
                error_kind = error.code(),
                "Background worker failed"
            );
        }
        if *stopped.borrow() {
            break;
        }
    }
    tracing::info!(event = "worker_stopped", "Indexing worker stopped");
}

async fn artwork_worker(
    catalog: hito_core::Catalog,
    mut stopped: tokio::sync::watch::Receiver<bool>,
) {
    tracing::info!(event = "artwork_worker_started", "Artwork worker started");
    let mut interval = tokio::time::interval(Duration::from_secs(1));
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        tokio::select! {
            biased;
            _ = stopped.changed() => break,
            _ = interval.tick() => {}
        }
        let started = Instant::now();
        let result = catalog
            .run_next_artwork_job_until(async {
                let _ = stopped.changed().await;
            })
            .await;
        match result {
            Ok(claimed) => tracing::debug!(
                event = "artwork_worker_polled",
                claimed,
                elapsed_ms = started.elapsed().as_millis() as u64,
                "Artwork worker checked pending work"
            ),
            Err(error) => tracing::error!(
                event = "artwork_worker_failed",
                error_kind = error.code(),
                elapsed_ms = started.elapsed().as_millis() as u64,
                "Artwork worker failed"
            ),
        }
        if *stopped.borrow() {
            break;
        }
    }
    tracing::info!(event = "artwork_worker_stopped", "Artwork worker stopped");
}

async fn follow_worker(
    collection: hito_core::Collection,
    mut stopped: tokio::sync::watch::Receiver<bool>,
) {
    tracing::info!(
        event = "follow_worker_started",
        "Playlist follow worker started"
    );
    let mut interval = tokio::time::interval(Duration::from_secs(1));
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        tokio::select! {
            biased;
            _ = stopped.changed() => break,
            _ = interval.tick() => {}
        }
        let started = Instant::now();
        let result = collection
            .run_next_follow_refresh_until(async {
                let _ = stopped.changed().await;
            })
            .await;
        match result {
            Ok(claimed) => tracing::debug!(
                event = "follow_worker_polled",
                claimed,
                elapsed_ms = started.elapsed().as_millis() as u64,
                "Playlist follow worker checked pending work"
            ),
            Err(error) => tracing::error!(
                event = "follow_worker_failed",
                error_kind = error.code(),
                elapsed_ms = started.elapsed().as_millis() as u64,
                "Playlist follow worker failed"
            ),
        }
        if *stopped.borrow() {
            break;
        }
    }
    tracing::info!(
        event = "follow_worker_stopped",
        "Playlist follow worker stopped"
    );
}
