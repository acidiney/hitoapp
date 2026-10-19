use tracing_subscriber::prelude::*;

pub(super) fn initialize() -> Result<(), ()> {
    let level = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));
    tracing_subscriber::registry()
        .with(tracing_subscriber::filter::filter_fn(|metadata| {
            let target = metadata.target();
            target == "hito_plugin_real_debrid"
                || target.starts_with("hito_plugin_real_debrid::")
                || target == "hito_remote_audio"
                || target.starts_with("hito_remote_audio::")
                || target == "hito_debrid_real_debrid"
                || target.starts_with("hito_debrid_real_debrid::")
        }))
        .with(level)
        .with(tracing_subscriber::fmt::layer().json())
        .try_init()
        .map_err(|_| ())
}
