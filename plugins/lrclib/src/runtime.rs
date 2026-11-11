mod settings;

use hito_plugin_lrclib::{Shutdown, router};
use hito_plugin_sdk::server::init_logging;
use std::{process::ExitCode, time::Instant};
use tracing::Instrument;

pub(super) async fn run() -> ExitCode {
    if std::env::args_os().len() == 2
        && std::env::args_os()
            .nth(1)
            .is_some_and(|value| value == "--help")
    {
        println!(
            "hito-plugin-lrclib --service-token-file /private/token [--listen 127.0.0.1:8101] [--endpoint http://127.0.0.1:PORT/]"
        );
        return ExitCode::SUCCESS;
    }
    if init_logging(&["hito_plugin_lrclib"]).is_err() {
        return ExitCode::FAILURE;
    }
    let started = Instant::now();
    let service_id = uuid::Uuid::new_v4();
    async move {
        match serve(started).await {
            Ok(()) => ExitCode::SUCCESS,
            Err(reason) => {
                tracing::error!(
                    event = "lrclib_service_failed",
                    outcome = "failed",
                    reason,
                    elapsed_ms = started.elapsed().as_millis() as u64
                );
                ExitCode::FAILURE
            }
        }
    }
    .instrument(tracing::info_span!("lrclib_service", %service_id))
    .await
}

async fn serve(started: Instant) -> Result<(), &'static str> {
    let settings = settings::Settings::read()?;
    let shutdown = shutdown_signal(settings.config.shutdown_handle(), started)
        .map_err(|_| "signal_setup_failed")?
        .in_current_span();
    let app = router(settings.config).map_err(|_| "initialization_failed")?;
    let listener = tokio::net::TcpListener::bind(settings.listen)
        .await
        .map_err(|_| "bind_failed")?;
    tracing::info!(
        event = "lrclib_service_ready",
        outcome = "ready",
        elapsed_ms = started.elapsed().as_millis() as u64
    );
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown)
        .await
        .map_err(|_| "serve_failed")?;
    tracing::info!(
        event = "lrclib_service_stopped",
        outcome = "stopped",
        elapsed_ms = started.elapsed().as_millis() as u64
    );
    Ok(())
}

fn shutdown_signal(
    shutdown: Shutdown,
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
        let signal =
            tokio::select! { _ = interrupt.recv() => "SIGINT", _ = terminate.recv() => "SIGTERM" };
        #[cfg(not(unix))]
        let signal = {
            let _ = tokio::signal::ctrl_c().await;
            "Ctrl+C"
        };
        tracing::info!(
            event = "lrclib_shutdown_requested",
            outcome = "cancelling",
            signal,
            elapsed_ms = started.elapsed().as_millis() as u64
        );
        shutdown.cancel();
    })
}
