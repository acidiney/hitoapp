use tracing_subscriber::prelude::*;

pub(super) fn initialize() -> Result<(), ()> {
    let level = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));
    tracing_subscriber::registry()
        .with(tracing_subscriber::filter::filter_fn(|metadata| {
            metadata.target() == "hito_community"
                || metadata.target().starts_with("hito_community::")
        }))
        .with(level)
        .with(tracing_subscriber::fmt::layer().json())
        .try_init()
        .map_err(|_| ())
}
