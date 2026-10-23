mod config;
mod logging;

use hito_community::{Service, router};
use std::{process::ExitCode, time::Instant};
use tracing::Instrument;

pub(super) async fn run() -> ExitCode {
    if logging::initialize().is_err() {
        return ExitCode::FAILURE;
    }
    let started = Instant::now();
    let service_id = uuid::Uuid::new_v4();
    let span = tracing::info_span!("community_service", %service_id);
    async move {
        match serve(started).await {
            Ok(()) => ExitCode::SUCCESS,
            Err(reason) => {
                tracing::error!(
                    event = "community_service_failed",
                    outcome = "failed",
                    reason,
                    elapsed_ms = started.elapsed().as_millis() as u64
                );
                ExitCode::FAILURE
            }
        }
    }
    .instrument(span)
    .await
}

async fn serve(started: Instant) -> Result<(), &'static str> {
    let settings = config::Settings::read()?;
    let shutdown = shutdown_signal(started)
        .map_err(|_| "signal_setup_failed")?
        .in_current_span();
    let service = Service::open(settings.community)
        .await
        .map_err(|_| "state_initialization_failed")?;
    let result = async {
        let listener = tokio::net::TcpListener::bind(settings.listen)
            .await
            .map_err(|_| "bind_failed")?;
        tracing::info!(
            event = "community_service_ready",
            outcome = "ready",
            elapsed_ms = started.elapsed().as_millis() as u64
        );
        axum::serve(listener, router(service.clone()))
            .with_graceful_shutdown(shutdown)
            .await
            .map_err(|_| "serve_failed")
    }
    .await;
    service.close().await;
    tracing::info!(
        event = "community_service_state_closed",
        outcome = "closed",
        elapsed_ms = started.elapsed().as_millis() as u64
    );
    result?;
    tracing::info!(
        event = "community_service_stopped",
        outcome = "stopped",
        elapsed_ms = started.elapsed().as_millis() as u64
    );
    Ok(())
}

fn shutdown_signal(
    started: Instant,
) -> std::io::Result<impl std::future::Future<Output = ()> + Send> {
    #[cfg(unix)]
    let (mut interrupt, mut terminate) = {
        use tokio::signal::unix::{SignalKind, signal};
        (
            signal(SignalKind::interrupt())?,
            signal(SignalKind::terminate())?,
        )
    };
    Ok(async move {
        #[cfg(unix)]
        let signal = tokio::select! {
            _ = interrupt.recv() => "SIGINT",
            _ = terminate.recv() => "SIGTERM",
        };
        #[cfg(not(unix))]
        let signal = {
            let _ = tokio::signal::ctrl_c().await;
            "Ctrl+C"
        };
        tracing::info!(
            event = "community_shutdown_requested",
            signal,
            outcome = "draining",
            elapsed_ms = started.elapsed().as_millis() as u64
        );
    })
}
