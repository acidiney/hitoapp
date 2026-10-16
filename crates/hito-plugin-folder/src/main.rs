use hito_plugin_folder::Folder;
use std::{net::SocketAddr, path::PathBuf};
use tracing_subscriber::prelude::*;

#[tokio::main]
async fn main() {
    tracing_subscriber::registry()
        .with(tracing_subscriber::filter::filter_fn(|metadata| {
            metadata.target() == "hito_plugin_folder"
                || metadata.target().starts_with("hito_plugin_folder::")
        }))
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .with(tracing_subscriber::fmt::layer().json())
        .init();
    if run().await.is_err() {
        tracing::error!(
            event = "folder_service_failed",
            outcome = "startup_failed",
            "Folder service could not start; check arguments, folder access and scan limits"
        );
        std::process::exit(1);
    }
}
async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let mut music = PathBuf::from("/demo");
    let mut listen: SocketAddr = "127.0.0.1:8090".parse()?;
    let mut arguments = std::env::args_os().skip(1);
    while let Some(argument) = arguments.next() {
        match argument.to_str() {
            Some("--music") => music = arguments.next().ok_or("Missing folder")?.into(),
            Some("--listen") => {
                listen = arguments
                    .next()
                    .and_then(|value| value.into_string().ok())
                    .ok_or("Missing address")?
                    .parse()?
            }
            _ => return Err("Use --music <folder> --listen <address>".into()),
        }
    }
    let folder = tokio::task::spawn_blocking(move || Folder::scan(&music)).await??;
    let listener = tokio::net::TcpListener::bind(listen).await?;
    tracing::info!(
        event = "folder_service_ready",
        outcome = "ready",
        "Music folder HTTP service ready"
    );
    axum::serve(listener, folder.router())
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;
    tracing::info!(event = "folder_service_stopped", outcome = "stopped");
    Ok(())
}
