use hito_plugin_listenbrainz::{Config, router};
use std::process::ExitCode;

#[tokio::main]
async fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .json()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();
    let listen = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "127.0.0.1:8095".into());
    let Ok(config) = Config::production() else {
        return ExitCode::FAILURE;
    };
    let Ok(app) = router(config) else {
        return ExitCode::FAILURE;
    };
    let Ok(listener) = tokio::net::TcpListener::bind(&listen).await else {
        tracing::error!(event = "listenbrainz_bind_failed", outcome = "failed");
        return ExitCode::FAILURE;
    };
    tracing::info!(event = "listenbrainz_service_ready", outcome = "ready");
    match axum::serve(listener, app).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(_) => ExitCode::FAILURE,
    }
}
