#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let config = hito_server::RuntimeConfig::from_env().await?;
    hito_server::run(config, shutdown_signal()?).await
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
            event = "shutdown_signal",
            signal,
            "Shutdown signal received"
        );
    })
}
