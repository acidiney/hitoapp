use crate::{RuntimeConfig, config::RuntimeResult, router_with_config};
use std::{future::Future, time::Duration};

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
    let (stop, mut stopped) = tokio::sync::watch::channel(false);
    let indexer = services.indexer.clone();
    let worker = tokio::spawn(async move {
        tracing::info!(event = "worker_started", "Indexing worker started");
        let mut interval = tokio::time::interval(Duration::from_secs(1));
        loop {
            tokio::select! {
                biased;
                _ = stopped.changed() => break,
                _ = interval.tick() => {
                    if let Err(error) = indexer.run_next_job().await {
                        tracing::error!(event = "worker_failed", error_kind = error.code(), "Background worker failed");
                    }
                }
            }
        }
        tracing::info!(event = "worker_stopped", "Indexing worker stopped");
    });
    tracing::info!(event = "server_listening", address = %listener.local_addr()?, "Hito is listening");
    let result = axum::serve(
        listener,
        router_with_config(services, config.http)
            .into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .with_graceful_shutdown(async move {
        shutdown.await;
        tracing::info!(event = "shutdown_requested", "Shutdown requested");
    })
    .await;
    let _ = stop.send(true);
    worker.await?;
    tracing::info!(event = "server_stopped", "Hito stopped");
    Ok(result?)
}
