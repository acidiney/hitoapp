use hito_plugin_sdk::server::init_logging;
use hito_plugin_spotify::{Config, router};
use std::{net::SocketAddr, time::Instant};

#[tokio::main]
async fn main() {
    init_logging(&["hito_plugin_spotify"]).expect("logging is initialised once");
    if let Err(reason) = run().await {
        tracing::error!(
            event = "spotify_service_failed",
            outcome = "failed",
            reason,
            "Spotify playlist service could not run"
        );
        std::process::exit(1);
    }
}

async fn run() -> Result<(), &'static str> {
    let started = Instant::now();
    let shutdown = shutdown_signal().map_err(|_| "signal_setup_failed")?;
    let mut listen = SocketAddr::from(([127, 0, 0, 1], 8092));
    let mut arguments = std::env::args_os().skip(1);
    while let Some(argument) = arguments.next() {
        match argument.to_str() {
            Some("--listen") => {
                listen = arguments
                    .next()
                    .and_then(|value| value.into_string().ok())
                    .ok_or("missing_listen_address")?
                    .parse()
                    .map_err(|_| "invalid_listen_address")?;
            }
            _ => return Err("invalid_arguments"),
        }
    }
    let app = router(Config::default()).map_err(|_| "initialization_failed")?;
    let listener = tokio::net::TcpListener::bind(listen)
        .await
        .map_err(|_| "bind_failed")?;
    tracing::info!(
        event = "spotify_service_ready",
        outcome = "ready",
        elapsed_ms = started.elapsed().as_millis() as u64,
        "Spotify playlist HTTP service ready"
    );
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown)
        .await
        .map_err(|_| "serve_failed")?;
    tracing::info!(
        event = "spotify_service_stopped",
        outcome = "stopped",
        elapsed_ms = started.elapsed().as_millis() as u64
    );
    Ok(())
}

fn shutdown_signal() -> std::io::Result<impl std::future::Future<Output = ()> + Send> {
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
            event = "spotify_shutdown_requested",
            signal,
            outcome = "draining",
            "Spotify playlist service shutdown requested"
        );
    })
}
