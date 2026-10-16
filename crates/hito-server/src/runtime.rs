use crate::{RuntimeConfig, config::RuntimeResult, router_with_config};
use std::{future::Future, time::Duration};
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
    let storage = services.storage.clone();
    serve(
        listener,
        router_with_config(services, config.http),
        storage,
        shutdown,
    )
    .await
}

pub async fn serve(
    listener: tokio::net::TcpListener,
    app: axum::Router,
    indexer: hito_core::LibraryStorage,
    shutdown: impl Future<Output = ()> + Send + 'static,
) -> RuntimeResult<()> {
    let (cancel, cancelled) = tokio::sync::oneshot::channel::<()>();
    let serving = tokio::spawn(
        serve_owned(listener, app, indexer, async move {
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
    indexer: hito_core::LibraryStorage,
    shutdown: impl Future<Output = ()> + Send + 'static,
) -> RuntimeResult<()> {
    let (stop, stopped) = tokio::sync::watch::channel(false);
    let worker = worker(indexer.clone(), stopped);
    let http = crate::connections::serve(listener, app, stop, shutdown);
    let (result, ()) = tokio::join!(http, worker);
    indexer.finish_indexing().await?;
    tracing::info!(event = "server_stopped", "Hito stopped");
    result
}

async fn worker(
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
