mod arguments;
mod config;
mod http;
mod logging;
mod private_files;

use hito_plugin_torbox::Service;
use std::{process::ExitCode, time::Instant};
use tracing::Instrument;

pub(super) async fn run() -> ExitCode {
    if logging::initialize().is_err() {
        return ExitCode::FAILURE;
    }
    let started = Instant::now();
    let service_id = uuid::Uuid::new_v4();
    async move {
        match serve(started).await {
            Ok(()) => ExitCode::SUCCESS,
            Err(reason) => {
                tracing::error!(
                    event = "torbox_service_failed",
                    outcome = "failed",
                    reason,
                    elapsed_ms = started.elapsed().as_millis() as u64
                );
                ExitCode::FAILURE
            }
        }
    }
    .instrument(tracing::info_span!("torbox_service", %service_id))
    .await
}

async fn serve(started: Instant) -> Result<(), &'static str> {
    let settings = config::Settings::read()?;
    let shutdown = shutdown_signal(started)
        .map_err(|_| "signal_setup_failed")?
        .in_current_span();
    let service = Service::open(settings.provider)
        .await
        .map_err(|_| "state_initialization_failed")?;
    let served = async {
        let listener = tokio::net::TcpListener::bind(settings.listen)
            .await
            .map_err(|_| "bind_failed")?;
        tracing::info!(
            event = "torbox_service_ready",
            outcome = "ready",
            elapsed_ms = started.elapsed().as_millis() as u64
        );
        http::serve(listener, service.router(), shutdown).await
    }
    .await;
    let closed = service.close().await.map_err(|_| "state_close_failed");
    if closed.is_ok() {
        tracing::info!(
            event = "torbox_service_state_joined",
            outcome = "joined",
            elapsed_ms = started.elapsed().as_millis() as u64
        );
    }
    served.and(closed)?;
    tracing::info!(
        event = "torbox_service_stopped",
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
            event = "torbox_shutdown_requested",
            signal,
            outcome = "stopping",
            elapsed_ms = started.elapsed().as_millis() as u64
        );
    })
}
