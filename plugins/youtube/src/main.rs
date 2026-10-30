use hito_plugin_sdk::server::init_logging;
use hito_plugin_youtube::{Config, Service};
use std::{net::SocketAddr, path::PathBuf, process::ExitCode, time::Instant};

#[tokio::main]
async fn main() -> ExitCode {
    init_logging(&["hito_plugin_youtube"]).expect("logging is initialised once");
    match run().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(reason) => {
            tracing::error!(
                event = "youtube_service_failed",
                outcome = "failed",
                reason,
                "YouTube HTTP service could not run"
            );
            ExitCode::FAILURE
        }
    }
}

async fn run() -> Result<(), &'static str> {
    let started = Instant::now();
    let signal = shutdown_signal().map_err(|_| "signal_setup_failed")?;
    let mut listen = SocketAddr::from(([127, 0, 0, 1], 8093));
    let mut executable = PathBuf::from("/opt/yt-dlp/yt-dlp");
    let mut metadata_only = false;
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
            Some("--executable") => {
                executable = arguments
                    .next()
                    .map(PathBuf::from)
                    .ok_or("missing_executable")?;
            }
            Some("--metadata-only") => metadata_only = true,
            _ => return Err("invalid_arguments"),
        }
    }
    if !executable.is_absolute() {
        return Err("invalid_executable_path");
    }
    let listener = tokio::net::TcpListener::bind(listen)
        .await
        .map_err(|_| "bind_failed")?;
    let config = Config { executable };
    let service = if metadata_only {
        Service::metadata_only(config)
    } else {
        Service::new(config)
    };
    let router = service.router();
    let (drain, drained) = tokio::sync::oneshot::channel();
    let server = async move {
        axum::serve(listener, router)
            .with_graceful_shutdown(async move {
                let _ = drained.await;
            })
            .await
    };
    tokio::pin!(server);
    tracing::info!(
        event = "youtube_service_mode",
        mode = if metadata_only {
            "metadata_only"
        } else {
            "full"
        }
    );
    tracing::info!(
        event = "youtube_service_ready",
        outcome = "ready",
        elapsed_ms = started.elapsed().as_millis() as u64
    );
    let served = tokio::select! {
        signal = signal => {
            tracing::info!(
                event = "youtube_shutdown_requested",
                signal,
                outcome = "canceling",
                elapsed_ms = started.elapsed().as_millis() as u64
            );
            None
        },
        result = &mut server => Some(result),
    };
    let _ = drain.send(());
    let cleanup = service.shutdown().await;
    let served = match served {
        Some(result) => result,
        None => server.await,
    };
    cleanup.map_err(|_| "cleanup_failed")?;
    served.map_err(|_| "serve_failed")?;
    tracing::info!(
        event = "youtube_service_stopped",
        outcome = "stopped",
        elapsed_ms = started.elapsed().as_millis() as u64
    );
    Ok(())
}

fn shutdown_signal() -> std::io::Result<impl std::future::Future<Output = &'static str> + Send> {
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
        {
            tokio::select! {
                _ = interrupt.recv() => "SIGINT",
                _ = terminate.recv() => "SIGTERM",
            }
        }
        #[cfg(not(unix))]
        {
            let _ = tokio::signal::ctrl_c().await;
            "Ctrl+C"
        }
    })
}
