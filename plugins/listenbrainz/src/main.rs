use hito_plugin_listenbrainz::{Config, router};
use std::{net::SocketAddr, path::PathBuf, process::ExitCode};
use url::Url;

const MAX_TOKEN_FILE_BYTES: u64 = 4098;

struct Settings {
    listen: SocketAddr,
    config: Config,
}

fn settings() -> Result<Settings, &'static str> {
    let mut listen = SocketAddr::from(([127, 0, 0, 1], 8095));
    let (mut contact, mut token_file, mut endpoint) = (None, None::<PathBuf>, None::<Url>);
    let mut arguments = std::env::args().skip(1);
    while let Some(argument) = arguments.next() {
        let value = arguments.next().ok_or("missing_argument_value")?;
        match argument.as_str() {
            "--listen" => listen = value.parse().map_err(|_| "invalid_listen_address")?,
            "--contact" if contact.is_none() => contact = Some(value),
            "--service-token-file" if token_file.is_none() => token_file = Some(value.into()),
            "--endpoint" if endpoint.is_none() => {
                endpoint = Some(Url::parse(&value).map_err(|_| "invalid_endpoint")?)
            }
            _ => return Err("invalid_arguments"),
        }
    }
    let path = token_file.ok_or("missing_service_token_file")?;
    let metadata = std::fs::metadata(&path).map_err(|_| "service_token_unreadable")?;
    if !metadata.is_file() || metadata.len() > MAX_TOKEN_FILE_BYTES {
        return Err("service_token_invalid");
    }
    let token = std::fs::read_to_string(&path).map_err(|_| "service_token_unreadable")?;
    let config = Config::new(
        &contact.ok_or("missing_contact")?,
        token.trim_end_matches(['\r', '\n']),
    )
    .map_err(|_| "invalid_contact_or_service_token")?;
    let config = match endpoint {
        Some(endpoint) => config
            .with_endpoint(endpoint)
            .map_err(|_| "invalid_endpoint")?,
        None => config,
    };
    Ok(Settings { listen, config })
}

#[tokio::main]
async fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .json()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();
    let settings = match settings() {
        Ok(settings) => settings,
        Err(reason) => {
            tracing::error!(
                event = "listenbrainz_start_refused",
                outcome = "failed",
                reason
            );
            return ExitCode::FAILURE;
        }
    };
    let Ok(app) = router(settings.config) else {
        return ExitCode::FAILURE;
    };
    let Ok(listener) = tokio::net::TcpListener::bind(settings.listen).await else {
        tracing::error!(event = "listenbrainz_bind_failed", outcome = "failed");
        return ExitCode::FAILURE;
    };
    tracing::info!(event = "listenbrainz_service_ready", outcome = "ready");
    match axum::serve(listener, app).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(_) => ExitCode::FAILURE,
    }
}
